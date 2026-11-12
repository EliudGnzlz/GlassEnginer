# REGISTRO — Fase 3: Audio, buses y ducking

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE3.md`. Especificación: §Fase 3 + anexo §8 del consolidado.

**Verificación de cierre: 333 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 34 → **35** (`glass_audio_buses`).

## Entregado

- **Buses Master/Music/SFX** sobre Kira 0.10.8, sin cambiar de backend. Nuevo módulo `glass_audio::buses`: `BusId` y `BusMixer`, lógica pura y testeable (6 tests). El factor de duck es una **función pura del tiempo** (bajada 0,25 s, meseta, subida 0,4 s), así el espejo del manager y el backend calculan exactamente lo mismo sin compartir reloj.
- **Kira**: Music y SFX son sub-pistas hijas de la principal (Master = volumen de la pista principal) para el audio 2D. Las fuentes 3D no pueden colgar de un bus (su pista espacial cuelga del oyente), así que el volumen efectivo del bus escala cada sonido 3D al nacer y en cada tick del backend si cambió; el Master actúa sobre la mezcla final.
- **Comandos y manager**: Play2D/Play3D llevan `bus`; comandos nuevos `SetBusVolume`, `DuckBus`, `SetSourceOcclusion`. `AudioSystem` gana `play_2d_on_bus`/`play_3d_on_bus` (los clásicos delegan a SFX), `set_bus_volume`, `duck_bus`, `set_source_occlusion`, y un **espejo consultable** (`bus_snapshot`, `sources_snapshot`, `live_3d_sources`) que no pasa por la cola de audio. El hilo de audio llama `backend.tick(now)` tras drenar la cola.
- **NullBackend** con el mismo estado observable (su `BusMixer` propio): la VM no tiene dispositivo de audio y el test físico corre con él, así que buses/ducking/oclusión se verifican igual que en hardware real.
- **Oclusión (low-pass)**: cada fuente 3D nace con un `FilterBuilder` LowPass a 18 kHz (el `FilterHandle` de Kira 0.10.8 sí tiene `set_cutoff`, generado por macro). La app lanza cada 150 ms un rayo cámara→fuente con `cast_ray_masked` de la Fase 1; si algo bloquea la línea de visión, el cutoff baja a **700 Hz** (tween 150 ms) y vuelve a 18 kHz al despejarse. Solo se avisa al sistema de audio cuando el estado cambia.
- **Escenas**: `AudioSourceData.bus` (serde default `"sfx"`). Deuda menor anotada: el enrutado del bus desde los scripts C# de AudioSource.
- **MCP**: `glass_audio_buses` (consulta + acciones `set_volume`/`duck`) y `glass_play_audio` con args `bus` y `looping`.
- **Docs**: `docs/AUDIO.md` nuevo (Kimi): arquitectura, jerarquía de buses, curva de ducking, oclusión, uso e inspección.

## Test físico (VM, NullBackend; driver `fase1/driver_f3.py`, captura en `test-fisico3/`)

Escena `Fase3Audio`: cámara en (0, 1.6, 6), pared con collider en z=0, emisor A en (0, 1.5, −4) tras la pared y emisor B en (3, 1.5, 3) a la vista. Vía MCP (música `beep_440` en loop por Music, SFX y dos emisores 3D por SFX):

1. `glass_audio_buses`: los 3 buses a volumen 1,0 y las 4 fuentes con su bus correcto ✓.
2. **Oclusión automática**: la fuente A reporta `occluded: true` y la B `false`, sin intervención manual; la captura confirma la geometría (el marcador rojo asoma tras la pared, el verde está a la vista) ✓.
3. **Ducking medible**: `duck` sobre Music (factor 0,2, 6 s) → durante la ventana `effective_volume` de Music = **0,2** con `ducking: true`; tras la ventana vuelve a 1,0 con `ducking: false` ✓.
4. `set_volume` de SFX a 0,5 reflejado en el snapshot (base y efectivo) ✓.

## Incidencias

- **DeepSeek quemó dos paquetes enteros en razonamiento sin contenido** (20.992 y 12.696 tokens) y un tercero quedó truncado a mitad de los tests (los completó Rayo). El resto de la fase lo cargó Claude (4 paquetes, todos aprovechados). Regla reforzada: con DeepSeek, paquetes aún más pequeños o directos a Claude cuando el paquete es de pares finos.
- El color de malla en escenas es **`[f32; 3]`** (RGB): un color de 4 elementos hace fallar el parseo de la escena con un críptico «trailing characters» de serde. La escena de la fase nació con colores de 4 y costó una bisección encontrarlo.
- El `FilterHandle` de Kira no muestra `set_cutoff` en un grep de `pub fn` (lo genera la macro `handle_param_setters!`): verificar la API en el fuente completo antes de descartarla.

## Consumo (de los `out*.json` de la fase)

- **Claude: 41.094 tokens** (manager, backend Kira, tools MCP).
- **DeepSeek: 43.684 tokens**, la mayoría quemados (solo se aprovechó la base de `buses.rs`).
- **Kimi: 3.030 tokens** (docs/AUDIO.md).
