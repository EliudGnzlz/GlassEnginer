# Registro: limpieza completa de Clippy (2026-11-13)

Orden de D3m3tr10 (2026-11-13 08:40): arreglar la deuda de Clippy
señalada al cerrar el CI — todos los lints que dejaban el paso de
Clippy en rojo, arreglados de verdad, sin `#[allow]` de maquillaje.

## Resultado

`cargo clippy --workspace --all-targets --locked` con
`RUSTFLAGS="-D warnings"` (las flags exactas del CI): **exit 0,
cero lints en todo el workspace**. En el diff no hay ni un solo
`#[allow]` nuevo (verificado por grep: 0). Pipeline local completo
de `ci.yml` reproducido y en verde antes de pushear.

La deuda real era mayor que los «~15 de glass_core» visibles: la
corrida de Clippy se detenía en la primera crate con errores y el
resto quedaba oculto detrás. Al limpiar crate a crate afloró todo
lo acumulado desde la Fase 0. Total aproximado: **más de 200
diagnósticos** entre lib y tests de las 9 crates.

## Qué se limpió, por crate

- **glass_core (16 lib + ~10 en tests):** el nuevo
  `too_many_arguments` de `recolectar_eventos` se resolvió
  agrupando la ventana en `TramoClip { state, clip, prev, next,
  dur, bucle }` (firma a 3 argumentos, sin tocar la API pública:
  la función es privada); clamp manual → `clamp`;
  `chunks_exact(3)` → `as_chunks::<3>()`; división con guarda →
  `checked_div`; `Error::new(Other)` → `Error::other` + cierre
  redundante eliminado; `map_or` → `is_some_and`; `Default` de
  `UIDragData` derivado; bucle por índice → `enumerate`;
  patrón OR → rango `0..=2`; lifetimes elididos y alias
  `MapasValidados` en shader_graph; aserciones de rango →
  `RangeInclusive::contains`; inicializadores con
  `..Default::default()`; continuación de doc indentada;
  préstamo innecesario en un test de vfs.
- **glass_physics (26):** las exportaciones FFI que desreferencian
  punteros (`attach_capsule/cylinder`, `cast_ray`, `cast_sphere`,
  `overlap_sphere`) ahora son `unsafe extern "C"` con su sección
  `# Safety`; `# Safety` también en `get_linear_velocity` y
  `get_transform`; `match` → `unwrap_or_default`;
  `!(x > 0.0)` → `partial_cmp` con `matches!`;
  `cast_capsule` agrupa su barrido en `BarridoCapsula` (único
  llamante externo: la tool MCP, actualizado); `Default` para
  `PhysicsWorld`; `assert_eq!(.., false)` → `assert!(!..)`;
  un `if` anidado colapsado en un test.
- **glass_particles (5):** `# Safety` en sus 3 exportaciones FFI;
  `spawn` del pool agrupa el nacimiento en `NacimientoParticula`;
  bucle por índice → `enumerate`; el `pub use ffi::...` se movió
  antes del módulo de tests.
- **glass_audio (10):** `# Safety` en las 7 exportaciones FFI;
  `Default` para `NullBackend` y `AudioSystem`; el método de
  trait `AudioBackend::play_3d` (8 argumentos) agrupa en
  `Play3dParams` (trait, dos impls y el llamante actualizados).
- **glass_scripting (45):** `# Safety` en los **43 callbacks**
  `unsafe extern "C"` de bindings.rs, cada uno con el contrato de
  sus punteros reales (salidas, cadenas C, buffers con capacidad,
  lotes); `iter().any(==)` → `contains`; `Default` para
  `ScriptHost`.
- **glass_renderer (~100):** la mayoría mecánicos aplicados con
  la propia sugerencia de Clippy y revisados en el diff
  (auto-deref, `as_chunks`, `div_ceil`, `is_multiple_of`,
  tamaños de slice, ordenación por clave, constantes
  `FRAC_1_SQRT_2`, doc). Los estructurales:
  - Las pipelines comparten `MeshPipelineLayouts { shadow,
    point_lights, lm, ibl }`: `new_mesh_pipeline`,
    `_transparent`, `new_graph_pipeline`,
    `new_mesh_pipeline_impl` y `new_skinned_pipeline` bajan a
    5–7 argumentos (12 sitios de llamada actualizados).
  - `SwapchainContext::new` toma `SwapchainTarget`.
  - `AssetCache::insert_texture` toma `TextureUpload`.
  - `GlassRenderer::render_views` toma `FrameDraws` (las 8
    listas del frame); llamantes en glass_app y en
    `render_offscreen_scene` actualizados.
  - Alias de tipo: `StaticCaster`/`GraphCaster` (sustituyen las
    tuplas de `ShadowCasters`, reexportados en la raíz de la
    crate) y `UiAtlasPacking`.
  - `# Safety` en `create_buffer`, `begin_pass` y
    `begin_face_pass`; dos fragmentos de doc colgados,
    eliminados o reenganchados.
- **glass_mcp (13):** préstamos innecesarios en argumentos
  genéricos, `chunks_exact_mut` → `as_chunks_mut`, bloque →
  operador `?`, módulo de tests reubicado, negación sobre
  `partial_cmp` reescrita con `matches!`, inicializador de
  `UITextData` con `..Default::default()`.
- **glass_editor (8):** `if`s colapsables, `format!` inútiles,
  `map_or`, rangos → `contains`, bucle de ejes del docking →
  `iter_mut().enumerate()`.
- **glass_app (~35):** `GlassEngineApp::new` (12 argumentos)
  toma `LaunchFlags` con las 11 banderas de arranque;
  `avanzar_particulas` (10) toma `CamarasParticulas`;
  la salida de `avanzar_animadores_fsm` es
  `SalidaAvanceAnimadores`; alias `DescriptorCuerpo`,
  `UiDrawItem`, `PortalRenderLists`, `CandidataLuz` y
  `EntidadEmisora` para las tuplas anotadas en el cuerpo de
  `render_frame` y afines (reutilizando `StaticCaster`/
  `GraphCaster` del renderer); `if ..is_some()` + `unwrap()` →
  `if let Some(vp_game)`; `let range = range;` redundante
  eliminado; los dos `match` de teclas colapsados en el patrón
  del `if let`; bucles por índice → `enumerate`; doc de
  `ui_set_text` reenganchada.

## Notas

- Ningún lint se «resolvió» ocultándolo: no hay `#[allow]`
  nuevos en el diff (0 por grep) ni excepciones que justificar.
- Las exportaciones FFI marcadas `unsafe extern` no cambian el
  ABI ni el lado C#: el host las referencia como punteros a
  función, igual que antes.
- Los arreglos automáticos de `cargo clippy --fix` se limitaron
  a glass_renderer, glass_mcp, glass_editor y glass_app, y su
  diff se revisó línea a línea antes de la suite.
- Suite local tras la limpieza: builds con `-D warnings` en
  verde, C# 0 errores, tests del workspace todos `ok`
  (0 fallos). La verificación que manda es la corrida real de
  Actions (ver el cierre en el chat principal).

## Addenda (2026-11-13): el lint que solo veía el CI

La corrida #39 (`cb223cf`) dejó el paso de Clippy en rojo por
UN solo lint que la limpieza local no detectó: la VM trabajaba
con Rust 1.99.1 y el CI instala el stable del día (1.100), cuyo
Clippy marca `needless_range_loop` en
`crates/glass_renderer/src/context.rs` — el bucle que suma la
VRAM local iteraba `0..memory_heap_count` indexando
`memory_heaps[i]`. Arreglo: iterar
`memory_heaps.iter().take(memory_heap_count)` y usar el heap
directamente (sin `#[allow]`). La toolchain local se actualizó
a 1.100.0 y se repitió la verificación completa con ella:
Clippy del workspace exit 0, builds con `-D warnings` en
verde, C# 0 errores y 429/429 tests.
