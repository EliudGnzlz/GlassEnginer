# Lista de después — ítems registrados fuera de la cola actual

Cola actual: pasada de medios → Fase 9 → prueba integral → bloque online
(P2P con anfitrión). Estos ítems están **registrados, no en cola**: entran
solo por orden expresa de D3m3tr10.

## 1. Servidor dedicado autoritativo (50+)

Escalón posterior independiente, fuera del bloque online (salió de él por
precisión del 2026-11-12). Servidor headless autoritativo para 50+
jugadores, medido con bots antes de hablar de persistencia o cuentas.

## 2. Port del player a Android 10+ (API 29+) — pedido el 2026-11-12

**Solo el player/export, no el editor**: el editor se queda en PC; el juego
se exporta a APK/AAB.

**Lo favorable**: el núcleo es Rust + Vulkan (Android es Vulkan nativo) y
los sistemas (física, audio, animación, partículas, escena) son agnósticos
de plataforma. No es una reescritura.

**Frentes**:

- **Frente duro nº 1 — C# en Android**: el hosting de CoreCLR/scripts como
  en escritorio no es directo en Android (.NET for Android existe, pero la
  carga dinámica de ensamblados de juego hay que resolverla: AOT u otra
  ruta). Es la incógnita que decide la dificultad: el primer paso del
  bloque, cuando llegue, es un **spike** que renderice una escena en un
  dispositivo real y resuelva la ruta de scripting antes de comprometer el
  resto.
- **Cáscara**: Activity/NDK, superficie y ciclo de vida
  (pausa/reanudación, destrucción/recreación de superficie y swapchain),
  rutas y almacenamiento con scoped storage de Android 10+.
- **Entrada y UI**: táctil multitáctil, **controles virtuales en el
  Canvas** (joystick y botones como componentes UI reutilizables, no
  pegotes por juego), gamepad, áreas seguras/notch, ratios variados y
  **multiventana/pantalla dividida de Android** (resize en vivo; el Canvas
  Scaler de la Fase 5 es la base).
- **Texturas**: el pipeline BC es de escritorio; en Android toca
  transcodificar a **ASTC** (ETC2 como respaldo) en la exportación.
- **Rendimiento**: GPUs móviles tile-based — validar coste de
  sombras/pases y presupuestos térmicos en dispositivo real, no en la VM.

## 3. Módulo de animación: CERRADO para la beta (auditoría 2026-11-12)

El módulo de animación queda **cerrado para la beta** con este alcance
(auditado contra el código):

- FSM con AnyState, exit_time y triggers.
- Blend tree **1D**.
- Skinning GPU con clips glTF.
- Tools MCP del Animator.

**Fuera de la beta (Post-1.0, como ya estaba registrado):** blend
trees 2D/Direct, capas, AvatarMask, IK, root motion,
StateMachineBehaviours, OverrideController, Playables,
retargeting/Avatar, Blend Shapes (el importador los salta a
propósito) y edición de curvas/keyframes.

## 4. Eventos de animación — HECHO el 2026-11-13

Los eventos de animación eran el **único hueco barato candidato a
colarse antes de la beta**. D3m3tr10 ordenó el 2026-11-13 empezar
por ellos dentro del bloque de animación diferida, y están
**integrados y verificados** (registro:
`REGISTRO_EVENTOS_ANIMACION.md`; detalle en `docs/ANIMACION.md`):
eventos en estados y en hijos de blend tree, disparo puro en
`advance` (wrap, clamp, dt gigante = 1 ocurrencia), entrega a C#
por `DispatchAnimEvent` → `OnAnimationEvent` y telemetría MCP
(`recent_events` en `glass_animator_state`). Suite 419/419 y
prueba física con dos X Bot (blend y estado simple) disparando
pasos por ciclo.

### 4b. Evaluación del resto de la lista diferida (2026-11-13)

Evaluado contra el código tras integrar los eventos. Propuesta a
la espera de su decisión:

- **StateMachineBehaviours (OnStateEnter/OnStateExit en C#):
  APROBADO por D3m3tr10 el 2026-11-13 y HECHO el mismo día**
  (registro: `REGISTRO_ESTADOS_ANIMACION.md`). Detección por
  comparación entrada/salida en `advance` (cubre transiciones,
  Play/CrossFade y el alta inicial; notificación al arrancar el
  fundido), entrega espejo de los eventos y telemetría
  `recent_state_events`. Suite 426/426 y prueba física completa.
- **Blend trees 2D: HECHO el 2026-11-13** por orden expresa de
  D3m3tr10 (adelanto del post-beta; registro
  `REGISTRO_BLEND2D.md`). Pesos por distancia inversa al
  cuadrado (sin triangulación), eventos y exit_time por el mismo
  camino que el 1D; el tipo Direct queda fuera.
- **Root motion: HECHO el 2026-11-13** por orden expresa de
  D3m3tr10 (segundo del adelanto post-beta; registro
  `REGISTRO_ROOTMOTION.md`). Opt-in por estado, delta global de
  la raíz aplicado al transform y al cuerpo, pose fijada; v1
  solo en el plano XZ.
- **Capas + AvatarMask**: coste alto (mezcla por hueso y capa en
  el skinning): **Post-1.0**.
- **IK**: coste alto: **Post-1.0**.
- **Retargeting/Avatar**: coste alto: **Post-1.0**.
- **Blend Shapes**: coste medio-alto (morph targets en el pipeline
  de skinning; el importador hoy los salta a propósito):
  **Post-1.0**.
- **Edición de curvas/keyframes en el editor**: coste alto
  (subsistema de editor): **Post-1.0**.
- **Vista de huesos en el editor**: coste medio; sigue siendo el
  requisito previo del auto-rigging y un bloque propio post-beta.
- **Auto-rigging**: sigue **el último de la cola** (ver §5).

## 5. Requisitos del bloque de auto-rigging (el último de la cola)

El auto-rigging sigue siendo el último bloque de la cola. Requisitos
registrados hoy:

- **(a) Vista de huesos en el editor — requisito previo.** Hoy no
  existe nada: cero referencias a huesos/esqueleto en `glass_editor`
  (verificado por grep). Hace falta un modo para ver el esqueleto,
  seleccionar huesos y posarlos, antes de que el auto-rig tenga
  sentido.
- **(b) Rig de MANOS y dedos: auto-rig SECUNDARIO y opcional**
  (precisión de D3m3tr10, 2026-11-12): NO forma parte del auto-rig
  principal — será un **módulo aparte**, que el desarrollador usa
  solo si lo quiere. El runtime ya animaría esas articulaciones vía
  skinning; lo que falta es ese generador opcional.
- **(c) Herramienta de AUTORÍA de ragdolls** (separación decidida
  por D3m3tr10 el 2026-11-13): configurar cuerpos y articulaciones
  por hueso y generar el ragdoll desde el esqueleto, en el editor.
  Vive EN ESTE bloque — agrupada con el diseñador 3D y el
  auto-rigging — porque comparte el requisito previo (a): la vista
  de huesos. Del **sistema runtime** del ragdoll, el núcleo
  (cuerpos por hueso + articulaciones esféricas) ya está commiteado
  (`5c70427`); el resto (piel que sigue a los cuerpos, activación y
  tool MCP) quedó aparcado en esta misma cadena por decisión del
  2026-11-13: la demo FPS usa enemigo simple con animación de
  muerte, sin ragdoll. La herramienta autora el sistema cuando
  este se complete aquí.

## 6. Rendimiento multinúcleo — CANDIDATO PRE-BETA (evaluado el 2026-11-12)

D3m3tr10 pidió evaluarlo como candidato a entrar antes de la beta
(no es Post-1.0 firme). Evaluación completa en
`~/workspace/glass-engine/EVALUACION_MULTINUCLEO.md`. Alcance
acotado: (1) física multinúcleo — la llamada a Rapier pasa `None`
en el hueco del ThreadPool, basta la feature `parallel` + pool de
rayon; (2) partículas y transforms SoA en paralelo por trozos;
(3) spawn masivo tras perfilar (el diagnóstico de la discrepancia
551 s / 21 s a 30.000 esferas está en curso). Fuera: sistema de
trabajos genérico y backend GPU de partículas (Post-1.0).
Verificación obligatoria: las cifras reproducibles de la Fase 10
(caída a 100 m = 1,46 m) con el solver paralelo. Hueco natural:
tras la iluminación y antes del bloque online. Decisión final al
cerrar la escalera de esferas.

**Cierre de la sección 6 (2026-11-12)**: la Fase B se implementó
y pasó su puerta de adopción el mismo día (informe
`INFORME_FASE_B_MULTINUCLEO.md`, commit `3bfbb48`): eventos
ordenados, feature `parallel` con pool propio, partículas y
transforms en paralelo; suite en 386 tests. El bloque queda
cerrado; no ocupa hueco en la cola activa.

## 7. Online — herramienta y complemento (decisión 2026-11-12)

El bloque online SALE de la cola activa y cambia de naturaleza:
cuando llegue su momento se manejará como **herramienta y
complemento** del motor, no como bloque central, y su diseño
partirá de esa óptica. El documento `BLOQUE_ONLINE.md` queda como
referencia (P2P con anfitrión 4–8, replicación con el mínimo
desync posible); no se implementa nada por ahora. El servidor
dedicado 50+ de la sección 1 sigue siendo un escalón aparte y
posterior.

## 7. Paridad RB/CC — piezas de importancia BAJA (registradas, no descartadas)

Decisión de D3m3tr10 (2026-11-13 22:56): tras la demo FPS se
implementan las piezas ALTA y MEDIA de `AUDITORIA_RIGIDBODY_CC.md`
(bloque de paridad). Las de importancia BAJA quedan para después,
pero tenidas muy en cuenta — esta lista es su registro explícito:

**Rigidbody:** `angularDamping` · `maxDepenetrationVelocity` ·
`solverIterations` / `solverVelocityIterations` · `AddRelativeForce` ·
semántica exacta de `excludeLayers` / `includeLayers` · toggles de
`automaticCenterOfMass` / `automaticInertiaTensor` · `PublishTransform` ·
`ClosestPointOnBounds` · `GetAccumulatedForce` / `GetAccumulatedTorque`.

**CharacterController:** `minMoveDistance` · `center` desplazable ·
`SimpleMove` · `material` / `sharedMaterial` · `Raycast` /
`ClosestPoint` / `ClosestPointOnBounds` heredados de Collider.

**HECHO (2026-11-16, bloque RB/CC 100%):** todas las piezas de esta
sección están implementadas (RB-1..RB-4 + CC-1/CC-2; registro en
`REGISTRO_RBCC100.md`, re-auditoría en `AUDITORIA_RIGIDBODY_CC.md`:
RB 56/57 con 1 parcial — la carga Collision sin contacts[] — y
CC 24/25; el único hueco restante, `excludeLayers` del CC, no
formaba parte de esta lista).
