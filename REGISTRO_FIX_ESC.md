# Registro — Arreglo del Escape en modo editor (2026-11-16)

## Defecto

En modo editor, pulsar Escape cerraba TODA la aplicación. Lo hizo imposible el
«Esc cancela» del Grab de MeshForge (B-E1): el panel trae su cancelación
(`glass_editor/src/panels/mesh_tool.rs`, `read_mesh_key`: grab activo + Esc →
«Movimiento cancelado.»), pero nunca llegaba a ejecutarse.

Evidencia en vivo (veredicto físico de B-E, 2026-11-16): con un Grab activo, al
pulsar Esc el log registró «Tecla Escape presionada. Saliendo.»
(2026-11-16T03:19:30Z) y la app se cerró limpiamente.

## Causa raíz

`crates/glass_app/src/main.rs`, `window_event`: el brazo del match para
`NamedKey::Escape` llamaba a `event_loop.exit()` INCONDICIONALMENTE, sin
distinguir el modo. El evento sí llegaba antes a egui (el paso previo en modo
editor lo entrega a `egui_winit.on_window_event`), pero la app salía en el
mismo evento, antes de que el editor pudiera usar la tecla.

## Arreglo (de raíz)

El brazo de Escape lleva ahora la guarda `if self.is_standalone`:

- **Standalone/juego** (`is_standalone == true`): comportamiento intacto —
  Escape sale de la app, con su log.
- **Editor** (`is_standalone == false`): el brazo no dispara; el evento cae
  al catch-all del match y la app sigue viva. Escape queda como tecla de
  trabajo del editor (cancelaciones del panel y equivalentes, que ya
  recibieron el evento vía egui). La salida del editor es la vía normal:
  `CloseRequested` (cerrar ventana) o el menú.

Sin `#[allow]`, sin tocar el paso previo a egui ni ningún otro brazo.

## Verificación

- **Pipeline local exacto** de `.github/workflows/ci.yml` (log
  `~/workspace/.tmp/pipeline_esc2.log`): build workspace 0, clippy exacto 0,
  build `--no-default-features` 0, dotnet 0 warnings / 0 errores, tests
  604/604. Nota: la primera pasada (`pipeline_esc.log`) marcó 4 errores de
  clippy en `glass_editor` (2 lints reales: `needless_borrow` en
  `docking.rs:1403` y `field_reassign_with_default` en el helper de test
  `panel_con_prefs` de `mesh_tool.rs:1690`, preexistentes del trabajo de la
  herramienta 3D, no del arreglo). Se arreglaron de raíz y la segunda pasada
  quedó en 0.
- **Commit**: `9fb37fe` — «Motor: Escape ya no cierra el editor (solo sale en
  standalone)», 4 archivos, 52 inserciones / 6 borrados, pusheado a `main`.
- **CI real**: corrida GitHub Actions **43817162465**, `completed` /
  `success` sobre `9fb37fe`.
- **Consumo**: Opus (claude-opus-5-5) **3.987 tokens** (3.609 prompt + 378
  completion) por el paquete del par. Los arreglos de clippy fueron
  integración directa de Rayo, sin gasto de APIs.
- **Verificación física pendiente (chat principal)**: editor con el
  complemento activo → sesión → Grab con G → Esc → «Movimiento cancelado.» y
  la app sigue viva; Esc ya no cierra el editor en general. Hasta ese
  veredicto, el bloque no se da por cerrado del todo.
