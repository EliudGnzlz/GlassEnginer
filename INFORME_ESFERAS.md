# Informe final — Escalera de esferas (spawn/despawn masivo)

Fecha de cierre: 2026-11-13. Entorno: VM de trabajo (4 vCPU
compartidas, ~1,5 efectivas; 6 GB de RAM; render software
lavapipe). Las cifras de tiempo son de ESTA VM: en hardware real
serán mejores; las de memoria y las conclusiones de escalado son
del motor.

Método: script `SesionEsferas.cs` sobre la escena `PruebaEsferas`,
generación y borrado por lotes con frames intermedios (como haría
un generador procedural en tiempo real), midiendo RSS (pico y
residuo), tiempos de script y de reloj. Sesiones 1-3 y 7:
mononúcleo. Sesiones 8, 10 y 12: con la conversión multinúcleo
(Fase B). Sesión 11: intento de 1M previo al arreglo. Sesión 12:
repetición del escalón de 1M tras el arreglo.

## Resultados limpios

| Escalón | Generar | Borrar | Pico RSS | Residuo |
|---|---|---|---|---|
| 1.000 dinámicas ×3 ciclos | ~1,0 s/ciclo | ~0,35 s/ciclo | 279 MB | 279 MB (plano) |
| 10.000 dinámicas ×3 (mono) | 96-98 s/ciclo | ~16 s/ciclo | 362 MB | 354-361 MB (plano) |
| 10.000 dinámicas ×3 (multi) | 7,5-8,6 s/ciclo | ~12 s/ciclo | 362 MB | 350-362 MB (plano) |
| 100.000 sin física ×3 (mono) | ~45 s/ciclo | ~106 s/ciclo | 889 MB | 792-879 MB |
| 100.000 dinámicas (multi, VM libre) | 324,2 s | 470,5 s | 1.525 MB | 1.502 MB |
| **1.000.000 sin física (tras arreglo)** | **133,9 s** | **140,7 s** | **2.477 MB** | 2.294 MB |

Lecturas:

- Sin fugas: en las sesiones de 3 ciclos el residuo es plano
  entre ciclos. El residuo alto tras el primer ciclo es el patrón
  conocido del asignador (conserva la arena del pico); no crece.
- Coste por entidad sin física: ~2,2 MB por cada 1.000 entidades
  y ~0,13 ms de spawn. El millón cabe en ~2,5 GB: el techo de
  esta VM (6 GB) NO se alcanzó; el techo real del motor queda por
  encima del millón y no se establece aquí por falta de RAM, no
  por desborde del motor.
- 100.000 dinámicas sostenidas con física completa: generarlas
  cuesta ~324 s en esta VM (el paso de física sobre 100k cuerpos
  domina; es una carga extrema, no un escenario de juego).

## Defectos encontrados por la escalera y su arreglo verificado

La regla de la escalera: todo defecto que destape un escalón se
arregla y se re-verifica repitiendo el escalón.

1. **Spawn O(N²) con física** (destapado por la sesión 4/5).
   `spawn_entity` recomponía las matrices de mundo de TODA la
   escena por cada entidad creada (30.000 en una llamada: 551,6 s).
   Arreglo: matriz de mundo solo por la cadena de padres de la
   entidad nueva. Re-verificado: la misma sonda en 21,0 s (26×).

2. **Generación sin física superlineal** (destapado por la
   sesión 11: 1M no completó en 3.600 s). Diagnóstico medido por
   tramos (50k→350k): el coste marginal por entidad crecía con N
   (0,13 → 1,14 ms). Dos causas:
   - **Índice de entidades lineal**: `SceneState::get_entity` y
     `get_entity_mut` buscaban por barrido lineal sobre el Vec de
     entidades y `delete_entity` borraba con `Vec::remove`
     (memmove): spawn y despawn O(N) por operación. Arreglo:
     índice `id → posición` (HashMap) mantenido de forma
     incremental, con reconstrucción perezosa si código externo
     muta el Vec directamente, y borrado por `swap_remove`.
   - **Culling inoperante con primitivas**: las cotas retenidas
     solo se registraban para modelos importados; las primitivas
     integradas (esfera, cápsula, cilindro, plano) caían en el
     respaldo «no recortar» de ±1e6, así que NUNCA se descartaban:
     drawn == considered == N en cada frame, y la pasada de
     sombras las dibujaba también todas (sin filtrar ni por la
     caja de la luz). Arreglo: cotas calculadas desde los vértices
     al registrar cada primitiva, y filtrado de casters por el
     frustum de la luz (la caja ortográfica de ±25 m que sigue a
     la cámara; quien está fuera no puede proyectar en el mapa).
   Re-verificación (repetición del escalón): **la sesión 12
   completó el millón — generar 133,9 s, borrar 140,7 s, pico
   2.477 MB, sin desborde**; en la sesión de diagnóstico repetida,
   con 350.000 entidades tras la cámara: culled = 350.000,
   drawn = 0, sombras = 0 (antes: drawn = 350.000, sombras =
   350.000), y el frame bajó de ~75 s a ~2,5-3,5 s.
   Regresión: la sesión del campo de tiro de la Fase 10 se repitió
   con el arreglo — fases completas (luces 0-16, animación, cola,
   recargas), caída del francotirador a 100 m de 1,48/1,48 m
   (línea base 1,46-1,51) y cadena de barriles intacta; captura
   visual del campo sin cambios. El soak de 8 min de esa sesión se
   interrumpió por un reinicio del entorno (no del motor); el soak
   equivalente ya había pasado en la sesión de la Fase B.
   Suite: 390 tests en verde (386 + 4 nuevos de los arreglos).

## Veredicto mononúcleo vs multinúcleo por subsistema

Datos: sesiones gemelas de la escalera y sesiones del campo F10,
antes (mono) y después (Fase B) de la conversión.

- **Física (Rapier)**: multinúcleo GANA donde hay muchas islas
  pequeñas (10.000 dinámicas: el spawn+paso por ciclo mejoró ~×1,6
  el ciclo completo en una VM de ~1,5 CPU efectivas; en CPUs de
  más núcleos la ganancia crece) y es NEUTRA en el extremo de un
  único paso masivo de 100k cuerpos (324 s vs 318 s: dominan las
  partes secuenciales por diseño — integración y reconstrucción
  de pares —, no el solver). Sin regresión en ningún escalón y con
  determinismo verificado (hash ×10 y eventos ×20 en verde).
  Veredicto: paralelo, adoptado.
- **Partículas**: el paralelo (≥8.192 vivas) es bit a bit
  idéntico al secuencial y más rápido en emisiones grandes; por
  debajo del umbral, secuencial. Veredicto: híbrido por umbral,
  ya en producción.
- **Transforms**: el paralelo por niveles es bit a bit idéntico y
  lineal (0,77 µs/entidad a 350k); con fallback secuencial para
  escenas pequeñas o niveles estrechos. Veredicto: híbrido por
  umbral, ya en producción.
- **Spawn/despawn**: secuencial O(1) tras el arreglo del índice;
  no necesita paralelismo (0,13 ms/entidad). La física asociada a
  un spawn masivo sí se beneficia del solver paralelo.
- **Recolección de render**: secuencial. Con el culling operante,
  recorrer y descartar cuesta ~8 µs/entidad·frame: a 350.000
  entidades son ~2,5-3,5 s por frame en esta VM. Es el coste
  lineal residual: aceptable para generación por lotes, pero es
  el siguiente cuello si se quiere renderizar mundos de >100k
  entidades a 60 fps.

## Sistema híbrido: diseño propuesto (con los datos de la escalera)

El motor ya es híbrido en partículas y transforms. El diseño
completo, apoyado en las mediciones:

1. **Umbrales por conteo, medidos, no adivinados**: partículas
   8.192 vivas; transforms 8.192 entidades y ancho de nivel 512;
   cada umbral nace de un test diferencial bit a bit + una
   medición, y queda como constante nombrada junto al subsistema.
2. **Física con pool adaptativo**: pool de N−1 hilos (máx. 4) por
   defecto; si los cuerpos dinámicos activos bajan de ~500, el
   paso se ejecuta en un pool de 1 hilo (el coste de coordinar
   supera a la ganancia con islas pequeñas y pocas). La elección
   es por paso y sin cambiar la API. (Propuesto; la Fase B dejó el
   pool fijo y la puerta lo validó así.)
3. **Secuencial donde el orden ES el contrato**: emisión de
   partículas (RNG), compactación, eventos de contacto (se
   ordenan tras drenar el lote) y spawn/despawn (O(1), con la
   estructura de datos correcta no hay nada que paralelizar).
4. **Regla general**: paralelizar solo bucles sin dependencias
   entre elementos y con equivalencia bit a bit demostrada por
   test diferencial; todo lo demás, secuencial rápido. Así, una
   CPU de muchos núcleos se aprovecha donde rinde y el monohilo
   nunca paga coordinación inútil — que es exactamente el ideal
   buscado: multinúcleo sin saturar el monohilo.

## ¿Queda el motor en beta con esto?

Contra el alcance de beta definido (beta jugable: render,
físicas, audio, scripting C# estilo Unity, MCP nativo, editor y
launcher; fases 0-10 cerradas; demos escaparate y FPS):

- **El núcleo está en nivel beta**: las 11 fases están cerradas
  con test físico, la conversión multinúcleo pasó su puerta sin
  regresiones (386→390 tests) y la escalera procedural está
  completa hasta 1.000.000 de entidades — el rasgo que define al
  motor (crear y regenerar mundos sin saturar memoria ni
  procesos) está medido, no solo afirmado.
- **La etiqueta beta pública queda a dos bloques ya ordenados y
  en cola**: la sesión del editor (incluye un defecto conocido y
  localizado: el FOV de la mira no se presenta en el standalone
  con editor) y la integración de iluminación por perfiles. Con
  esos dos cierres, el alcance de beta definido queda cubierto
  por completo.

## Consumo de la escalera (tokens por proveedor)

Conversión y verificaciones: ver `INFORME_FASE_B_MULTINUCLEO.md`.
Arreglos de la escalera (índice de entidades + culling):
Claude Sonnet 5: 53.663 medidos (17.889 índice + 21.736 culling +
14.038 de un paquete rechazado por contexto incompleto, sin
código aprovechable). Claude Opus 5-5: 2 intentos sin medición
(timeout del cliente a los 115 s; se reasignó a Sonnet según la
regla de fallback). DeepSeek: diseño de transforms de la Fase A.
Kimi: 0.
