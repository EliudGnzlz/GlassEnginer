# Transforms en GlassEngine

## Resumen

La fase 7 optimiza la resolución de matrices de mundo mediante un almacén SoA y una pasada lineal en orden topológico. La fuente autoral sigue siendo `EntityData.transform`: posición, rotación Euler en grados —con cuaternión derivado— y escala. Escenas, editor y scripts C# conservan el mismo modelo y firma de acceso.

## Almacén SoA

`glass_core::transforms::TransformStore` mantiene arrays paralelos y contiguos para:

- Ids de entidad e índices de padre.
- Posiciones, rotaciones como cuaterniones y escalas.
- Matrices de mundo.
- Orden topológico, con cada padre antes que sus hijos, calculado durante la reconstrucción.

## Resolución en una pasada

`SceneState::compute_world_matrices()` usa la caché del almacén:

- Si cambian entidades o padres, detectado mediante comparación de vectores sin hash, reconstruye la estructura y el orden topológico.
- Si la estructura se mantiene, solo refresca los valores locales.
- Resuelve todas las matrices en una pasada lineal, sin `HashMap` en el bucle.
- Materializa el mapa id→matriz únicamente al devolver el resultado, preservando la firma histórica.

## Paridad y tests

Se conserva el comportamiento anterior: un padre inexistente convierte la entidad en raíz y un ciclo de parentesco se resuelve como raíz para evitar que entidades desaparezcan.

Los tests de equivalencia contra el algoritmo previo cubren escenas planas, cadenas, árboles, orden invertido en el vector de entidades y ciclos.

## Medición

Benchmark `crates/glass_core/examples/bench_transforms.rs`, compilación release y misma máquina. Tiempos por pasada:

| Caso | Antes | Después |
|---|---:|---:|
| 5000 entidades planas | 0,364 ms | 0,273 ms |
| 5000 entidades en cadenas de 10 | 0,474 ms | 0,346 ms |
| Árbol de 2000 entidades | 0,183 ms | 0,139 ms |
| Escena de 600 entidades | 0,054 ms | 0,040 ms |

La reducción es de alrededor del 25 % en todos los casos. La primera versión SoA era más lenta porque validaba la estructura con un `HashMap` en cada llamada; sustituir esa validación por comparación de vectores corrigió la regresión.

## Inspección MCP

La herramienta `glass_transform_stats`, alimentada por `glass_core::transform_stats`, expone entidades, matrices recomputadas, profundidad máxima, microsegundos de la última pasada y total de pasadas.

## Alcance

SoA exclusivamente para transforms. No se introduce un ECS general ni se reestructuran otros componentes.