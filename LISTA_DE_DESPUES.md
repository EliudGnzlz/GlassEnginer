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

## 4. Eventos de animación — hueco SEÑALADO, sin decisión

Los eventos de animación quedan registrados como el **único hueco
barato candidato a colarse antes de la beta**. Están señalados, sin
decisión de implementarlos todavía: requieren orden expresa.

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
