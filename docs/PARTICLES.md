# Sistema de partículas

## Resumen

La fase 8P incorpora **un único sistema de partículas CPU mínimo**, implementado en el crate `glass_particles`, sin dependencias del resto del motor. No existe un sistema GPU: queda para post-beta si las mediciones lo justifican.

## Pool SoA

El pool almacena los datos en arrays paralelos —posición, velocidad, edad, vida, tamaño y rotación— en lugar de una estructura por partícula. Las partículas muertas se eliminan mediante *swap-remove* y cada emisor impone su propio límite `max_particles`.

La disposición SoA se fijó tras medir: con 50.000 partículas y 600 pasos, el mismo paso de simulación fue un **20–26 % más rápido que en AoS**, con resultados idénticos. El benchmark está en `crates/glass_particles/examples/bench_pool.rs`.

## Emisor y simulación

- Emisión continua por tasa con arrastre fraccional, además de ráfagas.
- Formas `point`, `sphere`, `box` y `cone`; la dirección base es el «arriba» local de la entidad.
- En espacio **mundo**, la partícula queda libre al nacer; en espacio **local**, las partículas vivas siguen el delta del emisor.
- Integración Euler con gravedad configurable —un valor negativo produce flotación—, *drag* y vida definida por rango.
- RNG xorshift con semilla: la misma escena emite igual en cada arranque.

El componente de escena `ParticleEmitter` contiene el bloque `particles`. Todos sus campos tienen valores por defecto razonables que configuran un fuego. Si la configuración cambia en vivo, el pool se reinicia con la nueva configuración.

## Apariencia y renderer

La apariencia define un gradiente RGBA por vida, con fundido de entrada del 12 % y de salida del 25 %, tamaño por vida mediante un factor final y rotación del *billboard* por vida.

El renderer transparente dedicado usa un pipeline propio sin iluminación, color por vértice y un disco blando procedural en el fragmento, sin texturas. Realiza prueba de profundidad, pero no escritura. La CPU expande cada partícula a cuatro vértices y sube los *billboards* a buffers mapeados por vista.

En el modo *stretched*, el quad se alinea con la velocidad proyectada en pantalla y se alarga según `velocidad × stretch_factor`, orientado a efectos como trazadoras.

## Control en runtime

MCP y scripts C# comparten una cola de comandos que la aplicación drena cada frame:

- `glass_particle_emit`: `play`, `stop` y `burst`.
- C#: `ParticleSystem.Play()`, `Stop()` y `Emit(n)` mediante el puente propio.
- `glass_particle_emitter`: crea y configura el emisor de una entidad.

Ambas herramientas MCP son mutantes de la Command Queue.

## Presupuestos y medición

`glass_particle_stats` expone partículas vivas, emisores, draws —uno por vista—, coste de simulación en µs y overdraw estimado. El proxy de overdraw es el área en píxeles de los quads dividida por el área del viewport.

En la escena de cierre se midieron unas 1.925 partículas vivas sobre un tope de 2.000, 2 draws, aproximadamente 2,6 ms de simulación más expansión en build debug sobre Vulkan por software —en release es mucho menos— y un overdraw aproximado de 0,72.

## Fuera del MVP

Quedan para post-beta: partículas GPU, colisiones con el mundo, trails, luces por partícula, sub-emisores y texturas de partícula.