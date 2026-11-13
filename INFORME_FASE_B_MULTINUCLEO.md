# Fase B — Conversión multinúcleo: implementación y puerta de adopción

**Fecha**: 2026-11-12. **Marco**: `EVALUACION_MULTINUCLEO.md` e
`INFORME_FASE_A_MULTINUCLEO.md` (dictámenes de Opus 5-5 para la
física y DeepSeek Flash para partículas/transforms). **Orden**: la
escalera de esferas se detuvo y este bloque pasó a ser el trabajo
activo; la escalera se re-corre después como verificación.

## Piezas implementadas (en este orden)

1. **C — Eventos de contacto deterministas** (`glass_physics`,
   código de Opus 5-5): `collect_contact_events` drena primero todo
   el canal a un Vec, lo ordena con clave total (Started antes que
   Stopped; dentro del grupo, por par crudo ordenado (índice,
   generación)) y procesa con la misma máquina de estados de
   siempre. Sin esto, la narrow phase paralela podía producir
   Enter/Exit espurios en entidades multi-collider. Válido y activo
   con y sin la feature.
2. **A+B — Física paralela** (código de Opus 5-5): feature
   `parallel = ["rapier3d/parallel", "dep:rayon"]` declarada en
   `glass_physics` (en `default`; reversible sin tocar el resto del
   motor) y pool propio de `PhysicsWorld` (hilos =
   `available_parallelism − 1`, acotado a [1, 4], override interno
   `GLASS_PHYSICS_THREADS`, nombres `glass-phys-N`). Cada
   `physics_pipeline.step` corre dentro de `pool.install(...)` con
   captura disjunta de campos; el post-paso sigue secuencial en el
   hilo llamante. Regla documentada en `step()`: ningún hook ni
   EventHandler propio puede llamar a C# (podría ejecutarse en un
   hilo de rayon). `enhanced-determinism` NO se tocó (desplaza la
   línea base). En Rapier 0.23.1 no hay ThreadPool que pasar: el
   `None` de la llamada es `query_pipeline` y se queda.
3. **Partículas** (código de Claude Sonnet 5 tras tres intentos
   fallidos de DeepSeek: dos contenidos vacíos y uno truncado):
   `Emitter::update` paraleliza el arrastre en espacio local y la
   integración Euler por encima de 8.192 vivas (zip de iteradores
   sobre campos SoA distintos) y compacta después en secuencial;
   emisión y compactación conservan el orden y el RNG exactos.
4. **TransformStore** (diseño de DeepSeek, código de Sonnet 5
   adaptado en la integración para eliminar el `unsafe` que traía):
   `compute_world` paraleliza por niveles de jerarquía (rangos
   contiguos de `depth_order`) calculando en paralelo a un Vec del
   nivel y volcando al cerrar el nivel; fallback a la pasada
   secuencial histórica si n < 8.192, si el nivel más ancho < 512,
   o si algún padre cae dentro del mismo nivel que su hijo (restos
   de ciclos). Mismo orden de operandos: resultados bit a bit.

Sin cambios en ninguna API pública. Sin sistema de trabajos
genérico ni backend GPU de partículas (Post-1.0, fuera del bloque).

## Puerta de adopción — resultados

- **Matriz de la suite**: 384/384 tests en las tres
  configuraciones (feature desactivada / `parallel` con 1 hilo /
  `parallel` con 3 hilos en esta VM).
- **Tests diferenciales nuevos** (quedan en la suite):
  partículas — supervivientes bit a bit contra el modelo
  secuencial con 20.000 vivas, muertes, drag, gravedad y arrastre
  local; transforms — mundo paralelo contra el algoritmo histórico
  a 9.000 raíces y a un árbol de 10.000.
- **Determinismo ×10** (test de crate): 10 mundos idénticos (pila
  de 12 cajas + bola, 600 pasos) producen el mismo hash de bits de
  posiciones y velocidades con el solver paralelo. PASS.
- **Secuencia de eventos ×20** (test de crate): entidades
  multi-collider que caen y se retiran producen secuencias de
  ContactEvent idénticas en 20 mundos frescos. PASS.
  La suite queda en **386 tests**.
- **Sesión del campo de la Fase 10 con la build paralela**
  (pool de 3 hilos confirmado en el log): SESION FIN con los
  conteos del script **idénticos** a la sesión mononúcleo
  (`log_conteos` sin diferencias: disparos, cadena de barriles,
  móviles, eventos), casts y ciclos de cola idénticos, soak de
  8 min estable (RSS 229–247 MB, como antes). La caída del
  francotirador a 100 m: **1,48 y 1,51 m**, frente a 1,46/1,47 de
  la sesión de cierre y 1,44/1,48 de la primera sesión
  mononúcleo: la dispersión entre los dos disparos gemelos ya era
  de ±4 cm en mononúcleo; la media sube ~3 cm, dentro de esa
  dispersión, y el test de hash ×10 (la medida rigurosa) es bit a
  bit. Se adopta con esta nota.
- **Escalera «después», escalón de 10.000 dinámicas** (misma
  configuración que la sesión 2 «antes»): GENERADAS 7,5/8,6/7,6 s
  de cronómetro de script frente a 96,2/96,6/97,8 s; BORRADAS
  ~12 s frente a ~16 s; sesión total 1.038 s frente a 1.667 s;
  pico 362 MB idéntico y residuos planos (350/352/362). Ojo: la
  sesión 2 «antes» corría aún con el spawn O(N²), así que el ×12
  de GENERADAS mezcla el arreglo del spawn y la conversión; el
  dato limpio de la conversión a esta escala es el ciclo completo
  (×1,6 en una VM de ~1,5 CPU efectivas).
- **Escalón de 100.000 dinámicas: PENDIENTE de re-correr en VM
  libre.** La sesión esf9 generó el ciclo 0 en 379,8 s (frente a
  317,7 s del «antes» con spawn arreglado y mononúcleo), pero la
  corrida compartió la VM con otra suite de tests (contención
  mutua documentada) y la pausa ordenada la detuvo; esa marca no
  se usa como veredicto. Hipótesis a comprobar en la re-corrida:
  a 50.000 cuerpos por lote dominan las partes que quedaron
  secuenciales por diseño (integración, `rebuild_active_pairs`),
  no el solver.

## Consumo de la Fase B

- Claude Opus 5-5: **19.287 tokens** (código de la física:
  piezas C, A y B).
- Claude Sonnet 5: **23.058 tokens** (9.100 partículas +
  13.958 transforms, reasignados tras los fallos de DeepSeek).
- DeepSeek Flash: **45.360 tokens** en tres intentos fallidos
  (20.577 vacío + 12.310 truncado + 12.323 vacío) y 0 útiles en
  código; su dictamen de la Fase A sí se usó como diseño.
- Kimi: 0.
- Integración, tests de puerta y verificación física: Rayo.
