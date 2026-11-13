# REGISTRO — StateMachineBehaviours (2026-11-13)

Segundo ítem del bloque de animación diferida. Los eventos de
animación cerraron con su registro y la evaluación del resto de la
lista (`LISTA_DE_DESPUES.md` §4b); D3m3tr10 aprobó el 2026-11-13
que **StateMachineBehaviours entrara como compañero de los
eventos**, con el mismo patrón de entrega.

## Diseño

Detección por comparación dentro de `AnimatorData::advance`: se
captura el estado a la entrada del frame (y si era válido en el
controlador) y se compara con el estado a la salida, tras todo el
procesamiento de transiciones y peticiones. Un solo punto de
detección cubre:

- transiciones por condición (incluidas las de AnyState y las que
  interrumpen un fundido en curso: el cambio notificado es el neto
  del frame),
- peticiones `Play`/`CrossFade` (la cola `pending_play`),
- el **alta inicial**: el primer frame, con `current_state`
  vacío/no válido, emite solo la entrada al estado por defecto.

Semántica fijada (documentada en `docs/ANIMACION.md`): la
salida/entrada se notifican **al arrancar el fundido**, no al
completarse — el estado destino ya es el actual desde ese frame —,
y `Play` al estado ya activo no emite nada.

## Implementación

- **Núcleo** (`crates/glass_core/src/animator.rs`):
  `StateChangeEvent { state, entered }` y
  `AdvanceOutput.state_events`. La salida llega primero
  (`entered: false`) y la entrada después.
- **Entrega a C#** espejando `DispatchAnimEvent`: el host resuelve
  el export gestionado `DispatchStateEvent` con tolerancia (un
  ensamblado sin el export arranca igual, sin notificaciones),
  `ScriptHost::dispatch_state_event(entidad, estado, entró)`; la
  app drena los cambios tras el pase de la FSM, fuera del bloqueo
  de la escena. En C#, `RuntimeBridge.DispatchStateEvent` busca el
  behaviour y le invoca `GlassBehaviour.OnStateEnter(string)` u
  `OnStateExit(string)`, con excepciones capturadas.
- **Telemetría MCP**: `glass_core::animator_stats` registra
  también los cambios (anillo compartido, mismo nº de orden global
  que los eventos); `glass_animator_state` incluye
  `recent_state_events`.

## Verificación

- **7 tests unitarios nuevos** (`tests_estados`): alta inicial sin
  salida, ya-en-estado sin entrada, transición exit→enter en orden,
  frames siguientes sin duplicados al completar el fundido,
  CrossFade notifica al arrancar y no al completar, Play
  instantáneo en el mismo frame, AnyState por trigger desde Walk.
  **Suite completa: 426/426** (419 + 7).
  - Nota de integración: el primer intento del test de AnyState
    falló por expectativa mía, no por el código — desde Walk sin
    fijar `walk`, la transición Walk→Idle (declarada antes)
    disparaba primero; el test fija `walk=1` para aislar la de
    AnyState. Orden de declaración = prioridad, como en Unity.
- **Prueba física** (`PruebaEstadosAnim.scene.json`, X Bot con el
  controlador `xbot` y el guión `EstadoAnimTest.cs`, que reproduce
  las fases de la prueba de la Fase 4): secuencia recibida en C# y
  en la telemetría MCP, idéntica y completa — ENTER Idle (alta) →
  EXIT Idle/ENTER Walk (walk=true) → EXIT Walk/ENTER Idle
  (walk=false) → EXIT Idle/ENTER Walk (CrossFade 0,5 s, al
  arrancar) → EXIT Walk/ENTER Idle (la condición de salida, aún
  activa, devolvió la máquina; el `Play(Idle)` final no emitió
  nada por estar ya en Idle). 9 registros con su orden global.

## Consumo

- **DeepSeek Flash: 9.810 tokens útiles** (núcleo 3.785 · host+app
  4.362 · C# 1.663), sin entregas quemadas.
- Claude/Kimi/Opus: 0 — los paquetes salieron a la primera con
  DeepSeek; sin timeouts que lamentar esta vez.
- Los 7 tests, la telemetría, la escena, el guión, el driver y la
  documentación: trabajo propio.
