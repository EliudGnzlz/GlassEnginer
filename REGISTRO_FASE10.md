# Registro de la Fase 10 — Demo FPS + prueba integral fundida

**Cerrada el 2026-11-12.** Informe completo: `INFORME_FASE10.md`.
Suite: **382 tests, 0 fallos**; build gestionado en verde
(0 errores, 0 avisos). Entorno de medición: VM con Vulkan por
software (lavapipe), build de desarrollo — los tiempos absolutos
no representan a una GPU real; valen las comparaciones entre
escalones y la corrección funcional.

## Lo entregado

- **Demo FPS** (escena `Fase10CampoTiro`, 402 entidades): armas
  **por datos** (`assets/data/armas.json`: pistola, AR hitscan,
  escopeta de 8 perdigones, francotirador, RPG), dianas a
  10/25/50/100 m, blancos móviles, barriles explosivos con cadena,
  mira con zoom por FOV y explosiones con impulso físico. Puentes
  nuevos: `RuntimeBridge.SetCameraFov` y `SetFog`.
- **Prueba integral fundida** en la misma sesión: física completa,
  PBR/toon, LOD, audio, UI, partículas, cola de comandos MCP
  (5 ciclos × 200/200 sin fuga), soak de 8 min sin crecimiento de
  RSS (239 → 241 MB) y ciclo día/noche.
- **Serie de luces 0/4/8/12/16 en las dos variantes**: con el tope
  real (8) y con la rama forzada a 16 (medida y **revertida**).
  Peor caso (noche, 16 reales): 285 ms de GPU en lavapipe.
  Recomendación registrada: mantener 8 por defecto en la beta; el
  coste de 16 es lineal (+26 %), no un precipicio.
- **Cúmulos de cubos contra el RPG**: 1.000 (completo), 5.000 y
  10.000 (asiento + disparo + explosión cosechados). Física de la
  pila despierta ~0,53 s/paso (1k) → ~1,67 (5k) → ~4,26 (10k);
  RSS estable tras la dispersión en los tres escalones.
- Documentación: `docs/FPS.md` (Kimi) y el README actualizado.

## Arreglos obligatorios de la fase (orden expresa: se arreglan, no se anotan)

1. **`Physics.OverlapSphere` truncaba a 64 en silencio** (buffer
   fijo del puente C#): la primera explosión del RPG solo empujó
   64 cubos. Arreglado (buffer creciente ×4 hasta 65.536) y
   **verificado a tres escalas**: la misma explosión pasó a
   afectar 1.002 cuerpos (1.000 cubos), 5.002 (5.000) y 9.382
   (10.000; el resto quedaba fuera de la esfera de 9 m).
2. **La re-exportación .blend resolvía rutas contra el `$PWD` del
   lanzador**: la recarga en caliente fallaba al abrir la app
   desde un acceso directo o un IDE. Arreglado en las tres capas
   (el handler MCP pasa rutas absolutas ancladas en el VFS; el
   vigía canoniza su carpeta y deriva el bridge de la raíz del
   proyecto; el bridge ancla las rutas relativas a su propia
   ubicación). **Prueba física**: recarga OK lanzando con un
   entorno hostil (`PWD=/tmp`), el escenario que antes fallaba.

## Defecto encontrado y localizado (arreglo pendiente)

En el binario **con editor** en `--standalone`, la vista
presentada sigue la posición y la rotación de la cámara de juego
pero **no su FOV** (la mira no se ve; medido ×1,1 frente al ×3,8
esperado). Con el binario de **juego puro** la misma sesión
muestra el zoom correctamente: el puente y el dato son correctos
y la divergencia está en la ruta de presentación del binario con
editor. Su arreglo encaja en la **sesión del editor**, el
siguiente bloque de la cola.

## Otros hallazgos registrados

- Convención de colliders de escena: salen de `box_extents` en
  unidades de mundo; la escala del transform es solo visual.
- Tunelización de proyectiles: a 260 m/s el proyectil avanza
  ~4,3 m por paso físico y atraviesa discos intermedios.
- Sesgo de pitch sin aislar: los impactos a 10 m caen ~6 cm por
  encima del centro apuntado de forma sistemática.
- La niebla heredada (25→70) tapaba las dianas a 100 m; la escena
  del campo queda con aire claro (60→220).

## About de GitHub (para pegar a mano; el conector no edita la ficha)

> Motor de videojuegos open source tipo Unity: núcleo Rust +
> Vulkan 1.3, scripting C# estilo Unity y MCP nativo para
> desarrollo asistido por IA.

## Consumo de APIs de la fase

Claude Opus 5-5: 11.545 tokens (scripts del FPS). Claude Sonnet 5:
4.118 (ProyectilRpg). DeepSeek: ~40.700 (scripts de sesión, con
dos paquetes fallidos incluidos). Kimi: 3.602 (docs/FPS) + 3.977
y 3.994 (README y su corrección).
