# Registro — Root motion (2026-11-13)

Segundo del par post-beta ordenado por D3m3tr10 (tras los blend
trees 2D). Diseño en `fase1/diseno_rootmotion.md` (fuera del
repo).

## Qué entra

- **Dato**: `StateDef.root_motion: bool` (`#[serde(default)]`).
  Sin el flag, comportamiento idéntico al anterior.
- **Librería** (`glass_renderer::skin`):
  - `root_node_index`: primera articulación cuyo padre no es
    articulación (en Mixamo, las caderas).
  - `root_global_translation`: traslación global del nodo raíz
    en la pose de un clip (la escala/rotación de la armadura
    quedan aplicadas: el resultado está en metros del modelo).
  - `RootMotionTracker`: desplazamiento mezclado entre frames
    desde los samples (clip, tiempo, peso), con desenvoltura de
    bucle por clip; un clip que entra aporta 0.
  - `evaluate_skin_matrices_multi_pinned`: como la multi, pero
    fija la traslación local de la raíz a su valor de reposo.
- **App** (`avanzar_animadores_fsm`): con el estado en root
  motion, el delta (rotado por el cuaternión de la entidad) se
  suma a `transform.position` y, si la entidad tiene cuerpo en
  el mundo físico global, se teletransporta con
  `set_transform`. El bucle de dibujo usa la evaluación fijada
  para esas entidades (`anim_root_pin`, mismo patrón que
  `anim_pose`). Al salir del estado, el seguidor se descarta.
- **Plano XZ**: la Y del clip no se aplica en v1 (decisión de
  la prueba física, abajo).
- **MCP**: `glass_animator_state` informa `position` y
  `root_motion` del estado actual.

## Verificación

- 3 tests nuevos en skin.rs: índice de raíz y traslación
  global a mitad de clip; el seguidor suma el ciclo entero con
  envoltura (0 → 0,5 → 0,9 → 0,1); la pose fijada mantiene la
  raíz en reposo frente a la libre. Suite: 435/435.
- **Prueba física PASS** (`PruebaRootMotion` +
  `rootmotion_test`/`rootmotion_control`, driver
  `fase1/driver_rootmotion.py`): el X Bot con root motion
  avanzó **46,62 m** en línea recta (+Z, su dirección de
  avance) con la Y exactamente en 0; el de control, con el
  mismo clip Walk y sin el flag, no se movió ni 0,0001 m.
  El Walk del X Bot desplaza ~1,79 m por ciclo (medido del
  .glb: cadera local Y 1,53 → 180,24 con armadura ×0,01).
- Hallazgo de la primera corrida: con la Y aplicada, el bot
  se hundía −17 cm en 46 m porque el clip de Mixamo no cierra
  el bucle en altura (deriva ~−0,7 cm/ciclo en el propio
  clip). De ahí el plano XZ de v1.
- Pipeline `ci.yml` reproducido en local con Rust 1.100:
  Clippy 0, builds 0, C# 0 errores, tests 435/435.

## Consumo de tokens

- DeepSeek: 6.714 (un paquete: skin.rs + campo en animator.rs;
  5 pares a la primera).
- Claude: 0 · Kimi: 0 · Opus: 0.
- La integración en la app, el ajuste de la envoltura
  (muestrear el fin del clip un pelo antes: `sample_local_pose`
  envuelve el tiempo y el fin exacto devolvía el inicio) y la
  prueba física son trabajo de integración de Rayo. Los dos
  avisos de Clippy de la primera pasada (argumentos de más en
  `avanzar_animadores_fsm` y un `or_insert_with`) se resolvieron
  agrupando los mapas del pase en la estructura
  `EstadoAnimadores`, sin `#[allow]` nuevos.
