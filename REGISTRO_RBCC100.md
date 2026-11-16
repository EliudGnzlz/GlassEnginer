# Registro — RB/CC al 100% (2026-11-16)

Alcance: todas las piezas BAJAS de `LISTA_DE_DESPUES.md` §7. Cierre por
sub-bloque con pipeline exacto + CI real + verificación física de Rayo.
Consumo acumulado por proveedor al final de cada sub-bloque.

## RB-1 — angularDamping + solverIterations (CERRADO código + CI)

- **Commit**: `26804bb` — «RB/CC 100% (RB-1): angularDamping y solverIterations del Rigidbody», 8 archivos, 304 inserciones.
- **Pila completa** Rust → FFI → C# → datos de escena, patrón `linear_damping`:
  `PhysicsWorld::set/get_angular_damping` y `set/get_solver_iterations`;
  FFI `glass_physics_set/get_angular_damping` y `glass_physics_set/get_solver_iterations`;
  callbacks al final de `PhysicsBridgeCallbacks` (host.rs) y de la struct C#
  (RuntimeBridge.cs); delegates, registro y métodos públicos; propiedades
  `AngularDamping` (float) y `SolverIterations` (int) en `RigidBody.cs`;
  campos `angular_damping` (defecto 0.0) y `solver_iterations` (defecto 4)
  en `PhysicsData`/`SpawnedBodySpec`, aplicados en `create_body` y en
  `rebuild_physics_from_scene` (alias `DescriptorCuerpo` ampliado a 7
  elementos — el build inicial falló por no ampliarlo; lección: al ampliar
  la tupla superficie hay que ampliar el alias).
- **Semántica**: `solverIterations` es el TOTAL del solver para el cuerpo
  (base 4 de Rapier + adicionales); `solverVelocityIterations` queda para
  RB-3 (valor guardado, manda el máximo con solverIterations — Rapier 0.23
  solo tiene un contador adicional por cuerpo).
- **Tests**: set/get de ambas + frenado angular comparativo en vuelo. Lección
  del test: un cuerpo sin collider no tiene masa/inercia y Rapier anula su
  velocidad angular en el primer paso — el test usa cuerpos con collider
  box y a y = 8 para no tocar el suelo en 60 pasos.
- **Pipeline local exacto** (`pipeline_rb1_run.log`, salida completa):
  build 0, clippy 0, no-default-features 0, dotnet 0, tests 36 suites ok
  (0 fallos). El pipeline viejo no guardaba la salida; `pipeline_fixes.sh`
  ahora vuelca todo a `pipeline_full.log`.
- **CI real**: corrida **43871538363**, `completed` / `success` sobre `26804bb`.
- **Consumo RB-1**: Opus (claude-opus-5-5) **37.585** (23.316 + 14.269).
- **Verificación física**: pendiente (camino real: script C# con un cuerpo
  girando con AngularDamping alto vs 0, y SolverIterations 10 vs 4).
