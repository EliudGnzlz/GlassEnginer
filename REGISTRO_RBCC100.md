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

## CC-1 — CERRADO (2026-11-16)

- **Commit `365e002`** (6 archivos, 412 inserciones) + este registro.
  **CI 43938994898 success.** Pipeline local exacto en verde (tras dos
  correcciones de integración de Rayo: una línea «ARCHIVO:» filtrada
  por el troceado de pares y `Time` cualificado como UnityEngine.Time).
- Contenido: **minMoveDistance** (la magnitud del movimiento pedido
  menor que el umbral descarta la llamada a Move entera, incluido el
  arrastre de plataforma de ese frame; defecto 0.0 = desactivado —
  Unity usa 0.001, no se cambia el comportamiento de los juegos ya
  hechos), **center** (desplaza la cápsula en nuevo − anterior
  moviendo el cuerpo cinemático como teleport_character; Position
  sigue siendo el centro de la cápsula) y **SimpleMove** (solo C#:
  velocidad m/s, ignora la Y pedida, acumulador vertical propio con la
  gravedad por defecto del mundo (0, −9.81, 0) — simplificación
  documentada: no hay FFI de gravedad mundial — y devuelve
  CollisionFlags). Las propiedades nuevas también se empujan desde
  Create() si se fijaron antes de crear el personaje. Puente: 4 campos
  tras character_bounds. 7 tests `cc1_*` en verde.
- **Consumo CC-1: Opus 71.628** (30.897 de una primera generación
  COMPLETA cuya respuesta se perdió con un reemplazo de la VM antes
  de poder extraerla + 40.731 de la regeneración).
  **Acumulado del bloque: Opus 304.071.**

## CC-2 — CERRADO (2026-11-16)

- **Commit `f60bd6d`** (7 archivos, 576 inserciones).
  **CI 43942101487 success.** Pipeline local exacto en verde.
- Contenido: **material/sharedMaterial** — el motor no tenía
  PhysicMaterial en ninguna capa; se crea la clase C# mínima
  (`Friction`/`Restitution`), `Material` instancia al primer acceso
  como Unity y `SharedMaterial` guarda la referencia viva; asignar
  cualquiera aplica los valores al collider de la cápsula
  (`set_friction`/`set_restitution` de Rapier) y los guarda en el
  CharacterState. Honestidad registrada en el XML: la cápsula es
  invisible al solver (GROUP_32/NONE), así que hoy los valores quedan
  aplicados al collider sin cambiar interacciones con otros cuerpos.
  **Raycast/ClosestPoint/ClosestPointOnBounds** heredados de
  Collider, contra la PROPIA cápsula (analíticos con parry:
  `cast_ray_and_get_normal` y `project_point` sobre
  `Capsule::new_y`; el de bounds es clamp sobre `character_bounds`).
  La firma de Raycast se adapta a la convención del motor
  (origin/direction como `Physics.Raycast`; no existe tipo `Ray`) y
  rellena `EntityId` en el RaycastHit. Puente: 5 campos tras
  `character_get_center`. 5 tests `cc2_*` en verde.
- **Consumo CC-2: Opus 49.422** (37.094 + 12.328).
  **Acumulado del bloque RB/CC 100%: Opus 353.493.**

## Re-auditoría y CIERRE DEL BLOQUE (2026-11-16)

- **Commit de arreglos + sonda `1adf72a`** (10 archivos, 488
  inserciones). **CI 43947376675 success.** Pipeline local exacto
  en verde.
- **Re-auditoría** (addenda en `AUDITORIA_RIGIDBODY_CC.md`):
  alcance ordenado del §7 de `LISTA_DE_DESPUES.md` al **100 %**
  (13/13 RB + 8/8 CC). Sobre las tablas completas de la auditoría
  (82 ítems): **RB 56/57 con 1 PARCIAL** (la carga `Collision`
  lleva point/normal/impulse/relativeVelocity, sin `contacts[]` ni
  referencias collider/rigidbody) → 99,1 % ponderado; **CC 24/25**
  → 96 % (único hueco: `excludeLayers` heredado, fuera del alcance
  del §7; el CC sí tiene `includeLayers`).
- **Sonda física** (escena `PruebaRbcc100` + script `Rbcc100Probe`,
  standalone por el camino real): **29/29 OK** tras los arreglos.
  La primera corrida destapó 3 defectos reales en código ya
  entregado, arreglados de raíz en `1adf72a`:
  (1) `PublishTransform` (RB-4) con la condición de éxito invertida
  (el puente usa 0 = OK): nunca publicaba en cuerpos de escena y en
  cuerpos de script escribía el TransformData por defecto;
  (2) ese defecto llevaba el cuaternión (0,0,0,0), que al
  normalizarse producía NaN y envenenaba el cuerpo — guardas de
  norma en `set_transform` y `create_rigid_body` de world.rs, con
  test de regresión `set_transform_rechaza_cuaternion_cero`;
  (3) `glass_physics_set_transform` devolvía `bool` Rust (1 byte)
  leído por el delegado C# como BOOL (4 bytes): un `false` se leía
  como `true` — convertido al patrón i32 (delegado `int`,
  RuntimeBridge compara != 0), demostrado con sonda nativa
  (norma2=0 recibido y C# seguía viendo true antes del cambio).
  Además, la `Position` cacheada del CC no seguía el desplazamiento
  de `center` (CC-1): el setter y el push de `Create()` ahora
  mueven la caché en el mismo delta.
- **Consumo total del bloque RB/CC 100%: Opus 353.493 tokens**
  (RB-1 37.585 · RB-2a 25.132 · RB-2b 34.279 · RB-3 73.377 ·
  RB-4 62.070 · CC-1 71.628 · CC-2 49.422). Los arreglos de la
  sonda y la re-auditoría fueron trabajo directo de Rayo (0 tokens
  de API).

## CCX — excludeLayers del CC (2026-11-16, orden posterior al cierre)

- **Orden de D3m3tr10 (17:58):** cerrar la única pieza fuera del §7
  y publicar la Release v0.10.16.1-beta sobre el HEAD resultante.
- **Commit `a4a1a24`** (8 archivos, 236 inserciones). **CI
  43967247043 success.** Pipeline local exacto en verde (con los 4
  tests nuevos `ccx_*`).
- **Diseño:** espejo de `includeLayers` — campo `exclude_mask`
  (defecto 0) en CharacterState; `cc_filter` aplica la efectiva
  `include & !exclude` en las tres sondas; FFI i32
  `glass_character_set/get_exclude_layers`; puente con los dos
  campos AL FINAL de PhysicsBridgeCallbacks; C#
  `CharacterController.ExcludeLayers` (int, defecto 0).
- **Verificación física:** sonda `PruebaRbcc100` extendida con
  defecto + set/get + fase 2 (la capa del suelo excluida ⇒ el
  personaje lo atraviesa: grounded = false, y = −4,9): **32/32 OK**,
  sin regresiones (diff de nombres contra la corrida de 29).
- **Re-auditoría:** CC **25/25 → 100 %** (addenda en
  `AUDITORIA_RIGIDBODY_CC.md`). RB sin cambios (56/57, 1 parcial).
- **Consumo CCX: Opus 25.928.** Acumulado RB/CC + CCX: **Opus
  379.421**.
