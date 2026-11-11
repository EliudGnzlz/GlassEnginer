# Perfilado

## Overlay «Frametime»

El overlay del editor desglosa el tiempo de cada fotograma:

- **Total**: duración completa del fotograma.
- **Update**: ejecución de los scripts C#.
- **Physics**: simulación de Rapier y sincronización con la escena.
- **Render**: trabajo CPU de grabación y envío del 3D y del postprocesado.
- **Present**: blit/egui y presentación en pantalla.
- **GPU**: timestamps Vulkan del pase principal `render_views`.

`avg` es la media y `p95` el percentil 95, ambos sobre las últimas 240
muestras. Un p95 alto con media estable indica picos puntuales, no un coste
sostenido.

La línea de memoria muestra el RSS del proceso y la memoria del allocator GPU.
El RSS incluye memoria fuera del allocator, por lo que ambos valores no son
directamente equivalentes.

## Herramientas MCP

- `glass_frame_timing`: devuelve las fases anteriores y sus estadísticas de
  frametime para inspeccionar el fotograma desde una herramienta externa.
- `glass_memory_stats`: devuelve RSS del proceso y estadísticas de memoria del
  allocator GPU.

En modo headless pueden responder **«sin muestras»** si no se están produciendo
fotogramas o no existen timestamps GPU disponibles; no implica por sí solo un
fallo del motor.

## Soak test

Desde la raíz del proyecto, tras compilar el juego:

