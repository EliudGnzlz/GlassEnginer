# Registro — CharacterController (bloque CC)

**Fecha:** 2026-11-13. **Motivo:** orden de D3m3tr10 (12:18) de
adelantar el CharacterController porque es importante para la demo
FPS: el motor no tenía ninguno (el JugadorFps escribía la posición a
mano, sin gravedad ni colisiones; Plat3D usaba una cápsula dinámica).

## Qué se construyó

- **Núcleo Rust** (`crates/glass_physics/src/character.rs`): registro
  de personajes por entidad sobre el `KinematicCharacterController`
  de Rapier 0.23. Cuerpo kinemático position-based + cápsula
  (fricción 0). API del mundo: `create_character`, `move_character`,
  `teleport_character`, `destroy_character`, `character_info`.
- **FFI** (`glass_character_create/move/teleport/destroy/info`) y
  registro al final de `PhysicsBridgeCallbacks` en el host, con la
  convención de tolerancia a punteros nulos.
- **Componente C#** `CharacterController`
  (`managed/GlassEngine.Core/Physics/CharacterController.cs`):
  `Radius` 0,35 · `Height` 1,8 · `SkinWidth` 0,02 · `StepOffset` 0,4 ·
  `SlopeLimit` 45° · `SnapDistance` 0,3; `Initialize`, `Move` (recibe
  el dt del caller; devuelve el desplazamiento aplicado y actualiza
  `Position`/`IsGrounded`/`IsSliding`), `Teleport`, `Destroy`,
  `RefreshInfo`.
- **Tool MCP `glass_character_state`** (la nº 50): posición del
  centro, `grounded` y `sliding` del mundo vivo.
- **Escena de prueba** `assets/scenes/PruebaCharacter.scene.json`
  (generador `fase1/gen_character.py`) y script
  `CharacterTest.cs`: pared, escalera de 8 peldaños de 0,35 m con
  plataforma a 2,8 m, caída y rampa del 30° (caja rotada).

## El problema del escalón y su solución (lo importante)

El autostep de Rapier **no subía el peldaño de 0,35 m** con la
presión de gravedad normal de un script (vy = −1 m/s), verificado
con tests de sondeo contra el mundo aislado:

1. La sonda de impactos mostró que el primer golpe de cada frame es
   el suelo a toi ≈ 0 (la cápsula descansa dentro de la distancia
   `offset`) y que, aun clasificando la cara del escalón como pared,
   el movimiento neto era ≈ 0.
2. La subida/descenso a mano (lift + avance + asentar) tampoco
   sirve: el avance por frame (0,027 m) no basta para apoyar la
   cápsula sobre la cima y el descenso la desliza hacia atrás por
   la arista.

Solución adoptada (en `move_character`, como Unity):

- Con los pies en el suelo el movimiento se **separa**: pasada
  horizontal (pendientes y deslizamiento) y pasada vertical
  (gravedad y snap). En el aire o saltando, una sola pasada.
- El autostep de Rapier queda **apagado**; el escalón lo resuelve
  esta capa con la receta de **muestra adelantada**: si el avance
  horizontal se bloquea, se lanza la cápsula hacia abajo desde
  `step_offset` por encima y 0,30 m por delante; si aterriza a una
  altura subible, el personaje se alza a esa altura y reintenta.
- Dos cortes finos, cada uno con su regresión:
  - Impacto a toi ≈ 0 = la muestra nace dentro del escalón
    (step_offset menor que su altura): se rechaza, o el personaje
    treparía step_offset por frame como una cremallera.
  - **Puerta de discontinuidad**: dos rayos (a 0,05 m y a
    radio+piel) deben discrepar más de 0,6 × step_offset; sin ella
    la muestra «sube» a saltos una rampa del 30° aunque el
    slopeLimit sea 20°.

## Pruebas

- **Regresión Rust** (`crates/glass_physics/tests/character_steps.rs`,
  4 tests): autostep con presión de gravedad (sube, cruza y cae al
  otro lado), bloqueo con stepOffset 0,05, rampa del 30° que sube
  con límite 45° y que **no** sube con límite 20°.
- **Prueba física en vivo** (driver MCP, escena PruebaCharacter):
  **8/8 RESULTADO OK** —
  reposo y=0,91 grounded · pared: se detiene en z=1,87 (cara 1,50 +
  radio 0,35 + piel 0,02, exacto) · desliza: z fijo en 1,87 mientras
  x baja a −2,83 · escalera: arriba en la plataforma (x=14,55,
  y=3,72) · con stepOffset 0,05 se bloquea en x=5,63 · caída de
  1,8 m: aterriza a y=3,72 grounded · rampa 30° con límite 45°:
  sube a y=2,39 · con límite 20°: se queda en y=0,92.
  Cruce por MCP: `glass_character_state(15)` → exists, grounded,
  posición coherente.

## Deuda v1 (anotada en el diseño)

Sin `center` ≠ 0, sin componente de escena en JSON (se crea desde
C#), sin empuje fino de cuerpos dinámicos y sin reescalado de la
cápsula al agacharse (v1.1).

## Consumo de APIs

DeepSeek (deepseek-flash): P1 núcleo Rust 7.181 · P2 puente y
componente C# 7.323 · P3 script de prueba 10.256 = **24.760**.
Claude 0 · Kimi 0. La integración, el diagnóstico del escalón y
los arreglos fueron trabajo directo de Rayo.

## Addenda 2026-11-13 — Criterios «mejor que Unity» (teletransporte y plataformas)

Orden suya (14:10): los dos modos de fallo señalados como
requisitos de diseño y pruebas de aceptación.

- **Teletransporte**: ya era operación de primera clase; ahora
  además limpia el suelo recordado (sin arrastre residual).
  Verificado en Rust (`teletransporte_limpia_el_arrastre`) y en
  vivo (`teletransporte_limpio`: x = −6,00 exacta tras saltar del
  ascensor en marcha al suelo).
- **Plataformas y ascensores NATIVOS**: `CharacterState` recuerda
  su suelo (cuerpo + translation, vía sonda descendente tras cada
  Move con grounded) y el siguiente Move aplica primero el delta
  de ese cuerpo: la plataforma lleva al personaje en horizontal y
  en vertical sin scripts; bajar caminando se suelta solo al
  cambiar el suelo bajo los pies. Solo traslación (la plataforma
  que rota queda fuera de v1; el controller no rota por diseño).
- Dos arreglos que la implementación destapó: (1) los colliders
  de Rapier solo se propagan desde los cuerpos al hacer step() y
  el personaje se mueve desde scripts antes del step —
  `move_character` sincroniza los colliders del cuerpo suelo
  antes de consultar; (2) la fricción cinemática de Rapier 0.23
  (sin interruptor) anula el empuje normal de separación y el
  barrido horizontal sobre un cuerpo cinemático se auto-bloqueaba
  en un bucle de impactos a toi 0 (sobre Fixed mueve, sobre
  Kinematic devolvía [0,0,0]): la pasada horizontal separada sale
  una piel por encima si el suelo es cinemático.
- Pruebas: `tests/character_platforms.rs` 4/4 (arrastre
  horizontal, ascensor sube/baja sin despegarse, bajada caminando
  con suelta, teletransporte limpio) + escena en vivo
  `PruebaPlataformasCC` con `PlataformasCCTest`: GLOBAL OK
  (viaje charX 2,98 frente a platX 3,00; ascensor a y=3,67 sin
  ningún frame en el aire). Suite completa 457/457.
- Consumo: Opus 5.5: 23.125 (núcleo 10.731 + tests 10.335 +
  diagnóstico de pasarela 2.059). El primer intento del paquete
  sin partir murió por el timeout de lectura de 120 s del helper
  de Claude (pasarela sana: 3,6 s en prueba corta); el arreglo
  fue partir el paquete, sin cambiar de proveedor.
