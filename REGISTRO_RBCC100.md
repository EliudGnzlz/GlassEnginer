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

## RB-2a — AddRelativeForce + GetAccumulatedForce/Torque (CERRADO código + CI)

- **Commit**: `e9f34a6` — 6 archivos, 323 inserciones.
- **Pila**: `world.add_relative_force` / `apply_relative_impulse` (rotan con
  `rb.rotation()`), `get_accumulated_force` / `get_accumulated_torque`
  (exponen `user_force`/`user_torque`); FFI con 3 floats por valor en
  entrada y buffer `[In, Out]` en salida (patrones de `apply_force` y
  `get_angular_velocity`); 4 campos al final de `PhysicsBridgeCallbacks` y
  de la struct C#; delegates, registro y métodos; `AddRelativeForce` con
  switch ForceMode espejo de `ApplyForce` y `GetAccumulatedForce/Torque`
  en `RigidBody.cs`.
- **Correcciones de integración de Rayo** (el paquete de Opus traía 3
  errores): (1) los tests suponían `create_rigid_body(RigidBodyKind, …)` y
  `add_box_collider(h, [0.5; 3])` — la firma real es
  `create_rigid_body(id, BodyType, pos, rot)` + `attach_box_collider(h,
  half, rest, fric)`; (2) el FFI usaba `with_world*` inexistentes — se
  reescribió con `get_global_physics_world()` + lock; (3) los delegates
  con `float*` + stackalloc se pasaron al patrón establecido del archivo
  (3 floats por valor en entrada, `[In, Out] float[]` en salida); el
  `VelocityChange` de `AddRelativeForce` usaba `Vector3.Transform` y
  `Rotation` inexistentes — ahora `GetTransform` + `RotateVector` (patrón
  de `AddRelativeTorque`).
- **Trampa del integrador**: re-correr `integrar.py` sobre un archivo ya
  integrado DUPLICA los pares cuyo NUEVO contiene al VIEJO (pasó en
  world.rs y en host.rs: campos duplicados E0124). Nunca re-correr el
  mismo archivo de pares; si un par falla, aplicarlo solo a mano.
- **Pipeline local exacto** (`pipeline_rb2a_run.log`): build 0, clippy 0,
  no-default-features 0, dotnet 0, tests 36 suites ok (0 fallos).
- **CI real**: corrida **43902462391**, `completed` / `success` sobre `e9f34a6`.
- **Consumo RB-2a**: Opus (claude-opus-5-5) **25.132** (19.268 + 5.864).
  Acumulado del bloque: Opus 62.717.

## RB-2b — maxDepenetrationVelocity (CERRADO código + CI)

- **Commit**: `4395707` — 8 archivos, 312 inserciones.
- **Pila**: `set/get/clear_max_depenetration_velocity` + `apply_depenetration_caps`
  en `PhysicsWorld`; FFI (set → i32, get → f32 con -1.0); 2 campos al
  final del puente; delegates, RuntimeBridge; propiedad
  `MaxDepenetrationVelocity` en `RigidBody.cs` (defecto sin límite;
  0/negativo quita el límite); campo `max_depenetration_velocity` en
  `PhysicsData`/`SpawnedBodySpec` (0.0 = sin límite), aplicado en
  `create_body` y en `rebuild_physics_from_scene` (DescriptorCuerpo a 8).
- **Hallazgo de raíz** (sonda con eprintln): la separación de Rapier es
  POSICIONAL — el cuerpo sale de una penetración de 0,3 m en 0,5 s con
  vy ≈ 0 en todo momento. Un clamp solo de velocidad NO frena nada (el
  test de comportamiento lo demostró: y = 0,4988 con el clamp puesto).
  El límite real se aplica sobre el DESPLAZAMIENTO de separación por
  substep (snapshot de posición al inicio del substep + exceso devuelto
  a lo largo de la normal del manifold penetrante), con una guarda
  adicional sobre la velocidad lineal. La normal de Rapier apunta de
  rigid_body1 a rigid_body2 (sonda: piso body1, caja body2, normal +Y).
- **Correcciones de integración de Rayo**: el paquete reconstruía el FFI
  de `get_angular_damping`, el delegate de `GetAngularVelocity` (bool en
  vez de int), la clase `RigidBody` (no es `: Component`) y varios
  anclajes de main.rs/scene.rs; el par de la tupla superficie no traía
  la desestructuración ni la aplicación en rebuild (las escribí yo) y
  el segundo constructor de `PhysicsData` (línea 1310) no se aplicaba
  por multi-coincidencia (a mano). El setter del FFI se pasó a i32 para
  seguir el patrón de RB-1 (los delegates bool necesitan MarshalAs.I1).
- **Pipeline local exacto** (`pipeline_rb2b_run.log`): build 0, clippy 0,
  no-default-features 0, dotnet 0, tests 36 suites ok (0 fallos).
- **CI real**: corrida **43923997752**, `completed` / `success` sobre `4395707`.
- **Consumo RB-2b**: Opus (claude-opus-5-5) **34.279** (26.033 + 8.246).
  Acumulado del bloque: Opus 96.996.

## RB-3 — excludeLayers/includeLayers + solverVelocityIterations (CERRADO código + CI)

- **Commit**: `6c384ff` — 8 archivos, 586 inserciones.
- **Capas**: include (defecto u32::MAX) y exclude (defecto 0) por cuerpo;
  al fijarlos se reescribe el filter de todos los colliders del cuerpo
  como base & include & ~exclude (membership intacto). La máscara base
  se registra al insertar el collider (los 7 sitios insert_with_parent);
  los colliders sin registro (cápsula CC, ragdoll) toman su filter
  actual como base la primera vez → fijar/quitar es idempotente. Los
  colliders añadidos después nacen ya filtrados. FFI con i64 para las
  máscaras (el bit 31 no cabe en i32); C# uint.
- **solverVelocityIterations**: guardado por cuerpo (defecto 4);
  additional = max(solver, velocity) − base al fijar cualquiera de las
  dos (decisión suya: manda el máximo; Rapier solo tiene un contador).
- **Pipeline local exacto** (`pipeline_rb3_run.log`): build 0, clippy 0,
  no-default-features 0, dotnet 0, tests 36 suites ok (7 tests rb3_*).
- **CI real**: corrida **43929328138**, `completed` / `success` sobre `6c384ff`.
- **Consumo RB-3**: Opus (claude-opus-5-5) **73.377** (55.342 + 18.035).
  Acumulado del bloque: Opus 170.373.
- Nota de integración: el integrador SOLO aplica pares con VIEJO de
  coincidencia única (los 7 pares idénticos de sitios de inserción se
  saltaron; los apliqué yo con replace exacto, 7/7).

## RB-4 — CERRADO (2026-11-16)

- **Commit `afe07d0`** (8 archivos, 507 inserciones) + este registro.
  **CI 43935543838 success.** Pipeline local exacto en verde (36 suites).
- Contenido: **toggles automaticCenterOfMass/automaticInertiaTensor**
  (Unity): la maquinaria de override de paridad gana una puerta —
  `com_override_activo`/`inertia_override_activo` devuelven el valor
  manual solo si el toggle está en false; la puerta se aplica en los 4
  lectores (apply paso 3, get_center_of_mass, effective_local_com,
  total_inertia). Fijar centerOfMass/inertiaTensor apaga el automático;
  Reset* lo reactiva; apagarlo sin valor manual congela el valor
  efectivo actual (snapshot). Nota honesta: si ambos componentes quedan
  automáticos, apply limpia el override y el manual se descarta (la
  documentación de C# dice exactamente eso). **PublishTransform**
  (solo C#): compone GetTransform(entidad) + SetPhysicsTransform.
  **ClosestPointOnBounds**: AABB mundial por collider (compute_aabb),
  clamp por componente, gana el más cercano; sin colliders, C#
  devuelve el punto sin cambios.
- **Decisión de integración de Rayo (hallazgo de Opus, confirmado):**
  en escena los toggles se aplican DESPUÉS de adjuntar los colliders y
  fijar la masa (create_body en sus dos salidas y rebuild), porque el
  snapshot sin colliders congelaría un COM sin geometría.
- Escena: `automatic_center_of_mass`/`automatic_inertia_tensor` en
  PhysicsData (serde default true); DescriptorCuerpo a 13 elementos
  (los 2 bool al final). 5 tests `rb4_*` en verde.
- **Consumo RB-4: Opus 62.070** (46.571 + 15.499).
  **Acumulado del bloque: Opus 232.443.**
