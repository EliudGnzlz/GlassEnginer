# REGISTRO — Fase 7: Transforms SoA

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE7.md`. Especificación: §Fase 7 + anexo §8 del consolidado (SoA solo en transforms; nada de ECS general).

**Verificación de cierre: 345 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 41 → **42** (`glass_transform_stats`).

## Entregado

- **`glass_core::transforms::TransformStore`**: arrays paralelos contiguos (ids, `parent_idx`, padres autorados, posiciones, rotaciones como cuaterniones, escalas, mundos) y un **orden topológico** calculado al reconstruir (por niveles desde las raíces; los ciclos se resuelven como raíz, paridad histórica).
- **`SceneState::compute_world_matrices()`** trabaja sobre la caché (campo `transform_store: RefCell<TransformStore>` en SceneState): estructura distinta → `rebuild`; si no, `refresh_locals` y **una pasada lineal** en orden topológico. La firma pública no cambia: ninguno de los 10+ llamadores (app y editor) se tocó. La fuente autoral sigue siendo `EntityData.transform`.
- **`glass_core::transform_stats`** (espejo del patrón de render/lighting stats): entidades, recomputadas, profundidad máxima, µs de la última pasada y total de pasadas; la tool `glass_transform_stats` lo expone.
- **Tests de equivalencia** contra el algoritmo histórico (copia de referencia en el test): escenas planas, cadenas, árbol, vector invertido y ciclo como raíz.
- **Benchmark** durable: `crates/glass_core/examples/bench_transforms.rs`.

## Benchmark antes/después (release, misma VM, `compute_world_matrices` completo)

| Escenario | Histórico | SoA | Mejora |
|---|---|---|---|
| 5000 planas | 0,364 ms | 0,273 ms | −25 % |
| 5000 en cadenas de 10 | 0,474 ms | 0,346 ms | −27 % |
| Árbol binario de 2000 | 0,183 ms | 0,139 ms | −24 % |
| Escena de 600 | 0,054 ms | 0,040 ms | −26 % |

Nota honesta: la primera versión SoA salió **más lenta** (+20–40 %) porque `structure_differs` construía un HashMap de ids en cada llamada. Se corrigió guardando los padres autorados en un array y comparando vectores (sin hash): de ahí las cifras finales. Queda registrado porque es la lección de la fase: la SoA no gana por los arrays solos, gana cuando el bucle caliente deja de hashear.

## Test físico (VM; driver `fase1/driver_f7.py`, captura en `test-fisico7/`)

- Escena Fase2Demo viva sobre transforms SoA: `glass_transform_stats` → 215 entidades, 215 recomputadas, 144 µs por pasada (build debug), pasadas acumulándose con el render.
- Estrés: `glass_batch_spawn_entities` crea 250 entidades de golpe → la tool reporta **465 entidades / 465 recomputadas / 372 µs**, y el render pasa de 205 consideradas y 87 dibujadas a **455 consideradas y 312 dibujadas**, con la escena visualmente intacta (niebla y luces puntuales de la demo en su sitio).

## Consumo (de los `out*.json` de la fase)

- **Claude: 20.209 tokens** (TransformStore + integración + tests; su VIEJO de la función quedó corto y el cuerpo histórico huérfano se retiró a mano, patrón ya conocido).
- **Kimi: 1.661 tokens** (docs/TRANSFORMS.md).
- DeepSeek: 0. La optimización de `structure_differs` y el benchmark fueron de Rayo.
