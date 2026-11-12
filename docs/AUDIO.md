# Sistema de audio

GlassEnginer usa **Kira 0.10.8 sobre CPAL** para reproducción, mezcla, buses, sonido espacial y efectos. El audio se ejecuta de forma asíncrona: el juego nunca espera al hilo de audio.

Si no hay dispositivo disponible, se activa un `NullBackend` con el mismo estado observable. Esto permite ejecutar servidores y pruebas de CI sin hardware de audio.

## Arquitectura

- Las llamadas a `AudioSystem` se encolan en una **cola lock-free de 512 elementos**.
- Un hilo de audio desacoplado consume la cola y ejecuta las operaciones en Kira.
- `beep_440` y `beep_880` son clips procedurales precargados.
- El resto de clips se carga por nombre desde `assets`, en formato WAV.
- Las fuentes vivas conservan su handle para operaciones posteriores, como cambiar su oclusión.

## Buses

La jerarquía de buses es:

```text
Master
├── Music
└── SFX
```

| Bus | Uso típico | Implementación |
|---|---|---|
| `Master` | Mezcla final completa | Pista principal de Kira |
| `Music` | Música y audio 2D asociado | Sub-pista hija de `Master` |
| `SFX` | Efectos y audio 2D general; bus clásico por defecto | Sub-pista hija de `Master` |

Cada bus tiene un volumen base entre `0.0` y `1.0`, con valor predeterminado `1.0`.

Las fuentes 3D no cuelgan de una pista de bus: en Kira, la pista espacial cuelga del oyente. El volumen efectivo del bus asignado escala el volumen de cada sonido 3D al nacer y se vuelve a aplicar en cada tick del backend. `Master` sí actúa directamente sobre la mezcla final completa.

## Ducking

El ducking reduce temporalmente el volumen de un bus, por ejemplo para atenuar la música durante un efecto o un diálogo.

```text
duck_bus(bus, amount, dur_secs)
```

- `bus`: bus afectado; solo `Music` y `SFX` admiten ducking. `Master` no.
- `amount`: factor destino entre `0` y `1`.
- `dur_secs`: duración total de la ventana, en segundos.

Curva aplicada:

```text
volumen
  base ───╲
           ╲ 0,25 s
            ╲
  amount     └──────── meseta ────────╱
                                      ╱ 0,4 s
                                     ╱
                                    └──── fin de dur_secs
```

La bajada dura `0,25 s`; después se mantiene el factor indicado. La recuperación dura `0,4 s` y termina al final de la ventana.

## Oclusión 3D

Cada fuente 3D incorpora un filtro paso bajo que nace abierto, con cutoff a **18 kHz**.

Cada **150 ms**, la aplicación lanza un rayo desde la cámara hasta la fuente usando la máscara de capas de la física:

- Si algo bloquea la línea de visión, el cutoff baja a **700 Hz** con una transición de **150 ms**.
- Al despejarse la línea, el cutoff vuelve a **18 kHz**.

También puede controlarse manualmente una fuente mediante:

```rust
audio.set_source_occlusion(handle, true);
```

## Uso desde código

API principal de `AudioSystem`:

```rust
audio.play_2d_on_bus(clip, volumen, pitch, loop, bus);
audio.play_3d_on_bus(clip, posicion, volumen, pitch, loop, bus);

audio.set_bus_volume(bus, volumen);
audio.duck_bus(bus, amount, dur_secs);
audio.set_source_occlusion(handle, ocluida);
```

`play_2d` y `play_3d` mantienen el comportamiento clásico y equivalen a reproducir en el bus `SFX`.

## Uso desde escenas

El componente `AudioSourceData` incluye el campo `bus`:

```yaml
AudioSourceData:
  bus: "sfx"
```

Valores admitidos en los datos de escena: `"sfx"` —valor predeterminado—, `"music"` y `"master"`.

## Inspección y control por MCP

### `glass_audio_buses`

Devuelve:

- La jerarquía de buses.
- Volumen base y volumen efectivo de cada bus.
- Estado de ducking.
- Fuentes vivas, con su bus, posición y estado de oclusión.

Además de inspeccionar, permite actuar sobre el sistema:

- `action=set_volume`: cambia el volumen de un bus.
- `action=duck`: inicia una ventana de ducking.

### `glass_play_audio`

Permite iniciar reproducción y acepta los parámetros `bus` y `looping`.

## Límites conocidos

- Las curvas de atenuación por distancia configurables (`MinDistance`, `MaxDistance` y `Spatial Blend`) quedan para después de la beta.
- El enrutado del bus desde los scripts C# de `AudioSource` queda como deuda menor. El dato ya está presente en la escena, pero todavía no se enruta desde esos scripts.