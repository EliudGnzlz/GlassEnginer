# Registro — Blend trees 2D (2026-11-13)

Adelanto post-beta por orden expresa de D3m3tr10 («ve aplicando
lo primero»: blend trees 2D antes que root motion; undo/redo sin
tocar). Diseño en `fase1/diseno_blend2d.md` (fuera del repo).

## Qué entra

- **Tipos** (`crates/glass_core/src/animator.rs`): `BlendChild2D
  { clip, position: [f32; 2], speed, events }` y `BlendTree2D
  { param_x, param_y, children }`, serializables. `StateDef`
  gana `blend2d: Option<BlendTree2D>` con `#[serde(default)]`:
  los controladores existentes parsean sin cambios.
  **Precedencia: blend2d > blend (1D) > clip.**
- **Pesos** (`blend_weights_2d`, función pura): distancia
  inversa al cuadrado (Shepard) normalizada; coincidencia con
  la posición de un hijo (d² ≤ 1e-12) ⇒ peso 1; poda de pesos
  ≤ 1e-3 y renormalización a suma 1. Sin triangulación: el
  freeform de Unity (Delaunay) queda fuera por coste/sin
  demanda, igual que el tipo Direct.
- **Un solo camino de código**: el helper `weighted_children`
  unifica 1D y 2D y lo usan `state_reference_clip` (exit_time =
  hijo dominante), `state_samples` y los dos bloques de
  recolección de eventos (estado actual y origen del fundido).
  Semántica de eventos idéntica a la del 1D: solo hijos con
  peso > 0, reloj propio por hijo y bucle.
- **Relojes e higiene**: `advance_blend_clocks` avanza los
  hijos de ambos árboles (todos, pesen o no); la poda de
  relojes incluye los clips 2D; el reloj del estado no se
  envuelve si hay blend2d.
- **MCP**: `glass_animator_state` informa del blend2d activo
  (`param_x`, `param_y`, `value_x`, `value_y`, hijos con
  `position`/`weight`/`time`), espejo del informe 1D.

## Verificación

- **Tests unitarios nuevos (3)**: `blend2d_pesos_basicos`
  (centro de 4 esquinas = 0,25; coincidencia exacta = 1; suma
  1; vacío y un hijo), `blend2d_mezcla_por_parametros`
  ((0,0) Idle, (1,1) tercios, (2,0) Walk por `advance`) y
  `evento_en_hijo_de_blend2d` (dispara con peso 1). Suite
  glass_core: 205 → 208.
- **Prueba física PASS** (`PruebaBlend2D.scene.json` +
  controlador `blend2d_test.animator.json`, X Bot con Idle@
  (0,0), Walk@(0,2) con evento y Armature@(2,0); driver
  `fase1/driver_blend2d.py`): informe 2D por MCP con
  param_x/param_y ✓; (0,0) Idle = 1 ✓; (0,2) Walk = 1 ✓;
  evento `PasoBlend2D` recibido (clip Walk, t = 0,2604 =
  0,25 × duración) ✓; (1,1) tercios exactos 0,3333 ✓;
  (2,0) Armature = 1 ✓.
- Pipeline de ci.yml reproducido en local con Rust 1.100 y
  corrida real de Actions verificada (ver el cierre en el chat
  principal).

## Consumo

- DeepSeek (deepseek-flash): **12.395 tokens** — un paquete con
  los 11 pares (tipos, pesos, helper, advance, eventos, MCP y
  tests), entregado a la primera.
- Claude 0 · Kimi 0 · Opus 0. La integración, los arreglos de
  literales de tests y la prueba física son de Rayo.
