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
