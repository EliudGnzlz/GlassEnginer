# REGISTRO — Fase 8P: Sistema de partículas CPU (MVP)

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE8P.md`. Especificación: §Fase 8P del consolidado.

**Verificación de cierre: 354 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`; el proyecto gestionado C# también compila). Tools MCP: 45 → **48** (`glass_particle_emitter`, `glass_particle_emit`, `glass_particle_stats`; las dos primeras, mutantes de la Command Queue).

## Benchmark previo (el plan lo exigía antes de fijar el layout)

Paso de simulación completo con 50.000 partículas × 600 pasos, misma VM: **AoS 127–135 ms, SoA 93–111 ms** con resultados idénticos → **el pool es SoA** (−20/26 %). El benchmark vive en el crate (`examples/bench_pool.rs`) y se re-ejecutó al cierre con las mismas cifras.

## Entregado

- **Crate `glass_particles`** (sin dependencias del motor): `Emitter` + `ParticlePool` SoA con muerte por swap-remove y tope por emisor; emisión por tasa con arrastre fraccional y ráfagas; formas point/sphere/box/cone; espacios mundo y local (delta del emisor); Euler con gravedad y drag; RNG xorshift con semilla (emisión determinista); gradiente RGBA con fades, tamaño y rotación por vida. Cola global de comandos (Play/Stop/Burst) y puente FFI para C#.
- **Datos de escena**: componente `ParticleEmitter` con bloque `particles` (todos los campos con default: un fuego razonable). Si el dato cambia en vivo, el pool se reinicia con la configuración nueva.
- **Renderer**: pipeline dedicado (`particles.vert/frag`): sin iluminación, color por vértice, disco blando procedural, mezcla alfa, depth test sin escritura, sin culling. La CPU expande los billboards por vista (la escena y el juego salen correctos con la base de cada cámara) y los sube a buffers mapeados que crecen por duplicación; un draw por vista. **Stretched billboard**: el quad se alinea con la velocidad proyectada y se alarga con `stretch_factor`.
- **App**: paso `avanzar_particulas` por frame (función libre sobre campos, patrón de los animadores) que simula, drena comandos, expande y registra presupuestos en `glass_core::particle_stats` (vivas, emisores, µs de simulación, draws, overdraw proxy).
- **C#**: `GlassEngine.Core.Particles.ParticleSystem` con `Play()/Stop()/Emit(n)`, puente `InitParticleBridge` tolerante (Init separado por feature, como audio/física) y las FFI `glass_particles_play/stop/burst` que encolan comandos: el script y el MCP comparten camino.

## Test físico (VM; driver `fase1/driver_f8p.py`, carpeta `test-fisico8p/`)

Escena `Fase8PParticulas`: fuego (cono, tasa 1.000, tope 2.000) y trazadora estirada horizontal.

- `glass_particle_stats`: **1.925 vivas**, 2 emisores, **2 draws** (uno por vista), simulación+expansión ≈ **2,6 ms en debug** sobre lavapipe, overdraw proxy 0,72.
- Captura: columna de fuego amarilla en la base que se apaga a brasas rojas arriba, y la **trazadora como una línea cian continua** cruzando el encuadre (los quads estirados se encadenan).
- `glass_particle_emit` stop sobre el fuego → las vivas caen a **14** (solo la trazadora); play de nuevo + burst de 60 en la trazadora → 1.616 y subiendo.
- El frame completo en debug/lavapipe va dominado por el render por software (total ~214 ms); el coste propio de partículas es el `sim_micros` de arriba.

## Deudas y límites del MVP

- En el editor, los billboards de la vista de escena se expanden con la base de la cámara orbital (aproximación implementada); en el build de juego ambas vistas comparten cámara y es exacto.
- El overdraw es un proxy con proyección nominal (1280×720, FOV 60°), no una medición de píxeles reales.
- Sin texturas de partícula (disco procedural), sin colisiones ni sub-emisores: post-beta según el plan.

## Consumo (de los `out*.json` de la fase)

- **DeepSeek: 13.892 tokens** (el crate completo con tests y benchmark).
- **Claude: 29.279 tokens** (pipeline del renderer y buffers; dos pares no casaron y se aplicaron a mano).
- **Kimi: 2.406 tokens** (docs/PARTICLES.md).
- Rayo: datos de escena, paso de la app, tools MCP, puente C#, contenido y test físico.
