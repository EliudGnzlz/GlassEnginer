# Auditoría Rigidbody y CharacterController — Unity ↔ GlassEnginer

**Fecha:** 2026-11-13 · **Método:** propiedad por propiedad y método por
método contra el código real (C# expuesto a scripts + FFI + mundo Rapier),
como las auditorías de iluminación y animación. Documento de referencia:
`analisis_completo_rigidbody_y_charactercontroller_unity.md` (Unity 6).
Su sección 4 presupone Jolt; este motor es **Rapier 0.23** y la
equivalencia se hace contra lo nuestro. Borrador de cruce: Kimi
(14.682 tokens); verificación y clasificaciones finales: Rayo.

**Corrección de premisa (2026-11-13):** la primera versión de esta
auditoría trataba el bloqueo de rotaciones como «decisión suya». No lo
era: fue una recomendación del asistente. Por orden suya del mismo día
el bloqueo global se retiró (commit `ede6bd1`) y pasó a ser opción por
cuerpo estilo Unity. Las filas angulares se clasifican contra el estado
YA desbloqueado; lo que falta ahí es hueco ordinario, no decisión.

## Rigidbody (57 ítems: 28 propiedades, 22 métodos, 7 callbacks/carga)

| Ítem Unity | Estado | Evidencia en GlassEnginer | Clase |
|---|---|---|---|
| `mass` | NO TIENE | `PhysicsData.mass` existe en `crates/glass_core/src/scene.rs` y en el Inspector, pero `SpawnedBodySpec` (`glass_app/src/main.rs`) no lo transporta y `create_rigid_body` no lo recibe. Sin `density` en `ColliderBuilder`: Rapier usa densidad 1.0 y masa = volumen. | HUECO-BETA |
| `linearDamping` | PARCIAL | Existe, pero fijo: `.linear_damping(1.5)` para todo dinámico en `crates/glass_physics/src/world.rs`; sin propiedad C# ni FFI. | HUECO-BETA |
| `angularDamping` | NO TIENE | Rapier lo soporta; no se fija ni se expone. | HUECO-POST |
| `useGravity` | NO TIENE | Gravedad mundial fija `[0, -9.81, 0]`; sin `gravity_scale` por cuerpo ni `Physics.gravity` expuesto. | HUECO-BETA |
| `isKinematic` | PARCIAL | `RigidBodyType { Dynamic, Static, Kinematic }` y `body_type` de escena lo eligen al crear; no se puede mutar tras `Initialize`. | HUECO-BETA |
| `position` | TIENE | `RigidBody.GetTransform/SetTransform`; FFI `get/set_transform`. | — |
| `rotation` | TIENE | En `GetTransform/SetTransform`; desde `ede6bd1` la física también PRODUCE rotación (antes solo se orientaba). | — |
| `linearVelocity` | TIENE | `GetLinearVelocity/SetLinearVelocity` en `Physics/RigidBody.cs`; FFI correspondiente. | — |
| `angularVelocity` | PARCIAL | Desde el desbloqueo Rapier la simula (los cuerpos giran y vuelcan), pero C# no expone leerla ni fijarla. | HUECO-POST |
| `maxLinearVelocity` | NO TIENE | Sin límite configurable en C#, FFI o `create_rigid_body`. | HUECO-POST |
| `maxAngularVelocity` | NO TIENE | Sin exponer. | HUECO-POST |
| `maxDepenetrationVelocity` | NO TIENE | No expuesto ni configurado al crear el cuerpo. | HUECO-POST |
| `automaticCenterOfMass` | PARCIAL | Rapier calcula el centro de masa desde los colliders siempre (equivale a «siempre true»); no se puede desactivar ni consultar. | HUECO-POST |
| `centerOfMass` | NO TIENE | Sin lectura ni escritura. | HUECO-POST |
| `worldCenterOfMass` | NO TIENE | Sin consulta expuesta. | HUECO-POST |
| `automaticInertiaTensor` | PARCIAL | Rapier deriva la inercia de los colliders automáticamente; no configurable ni consultable desde C#. | HUECO-POST |
| `inertiaTensor` | NO TIENE | Sin exponer. | HUECO-POST |
| `inertiaTensorRotation` | NO TIENE | Sin exponer. | HUECO-POST |
| `detectCollisions` | NO TIENE | Hay sensores (`is_trigger`), pero no un interruptor general de detección por cuerpo. | HUECO-POST |
| `collisionDetectionMode` | NO TIENE | Ningún cuerpo activa `ccd_enabled`; el `CCDSolver` del pipeline no equivale a CCD por cuerpo. Sin modos expuestos. | HUECO-BETA |
| `interpolation` | NO TIENE | El render usa el transform del último paso fijo 1/60; no hay par previo/actual ni interpolación expuesta. | HUECO-BETA |
| `constraints` | TIENE | Desde `ede6bd1`: bits por eje estilo Unity en Rust (`set_locked_axes`/`get_locked_axes`), FFI, `RigidBody.Constraints` y `PhysicsData.constraints` en escenas. | — |
| `freezeRotation` | TIENE | Propiedad `RigidBody.FreezeRotation` (atajo sobre los bits 8/16/32). | — |
| `excludeLayers` | PARCIAL | `layer`/`collision_mask` funcionan por matriz de capas; no existe la semántica aditiva de exclusión de Unity. | HUECO-POST |
| `includeLayers` | PARCIAL | `collision_mask` filtra por instancia; sin inclusiones que anulen exclusiones con la semántica de Unity. | HUECO-POST |
| `sleepThreshold` | NO TIENE | Rapier duerme cuerpos por defecto (`can_sleep`), pero el umbral no está expuesto. | HUECO-POST |
| `solverIterations` | NO TIENE | Sin configuración por cuerpo. | HUECO-POST |
| `solverVelocityIterations` | NO TIENE | Sin configuración por cuerpo. | HUECO-POST |
| `AddForce` | PARCIAL | `ApplyForce` (continua) y `ApplyImpulse` (instantáneo) cubren dos modos; no existe `ForceMode` ni `Acceleration`/`VelocityChange`. | HUECO-BETA |
| `AddRelativeForce` | NO TIENE | Sin variante en coordenadas locales. | HUECO-POST |
| `AddForceAtPosition` | NO TIENE | Fuerza/impulso sin punto de aplicación (con el desbloqueo, Rapier podría producir el torque resultante; falta la FFI). | HUECO-POST |
| `AddExplosionForce` | NO TIENE | Sin API radial ni `upwardsModifier`; el patrón existe compuesto en scripts de la Fase 10 (`OverlapSphere` + `ApplyImpulse` por cuerpo, cadena de barriles verificada), pero no es API. | HUECO-POST |
| `AddTorque` | NO TIENE | Rapier puede (`apply_torque`); sin FFI ni C#. | HUECO-POST |
| `AddRelativeTorque` | NO TIENE | Ídem en espacio local. | HUECO-POST |
| `MovePosition` | NO TIENE | `SetTransform` teletransporta; no calcula velocidad cinemática implícita para empujar dinámicos. | HUECO-BETA |
| `MoveRotation` | NO TIENE | Orientar con `SetTransform` no es movimiento rotacional cinemático con contactos. | HUECO-POST |
| `Move(pos, rot)` | NO TIENE | Sin operación cinemática atómica equivalente. | HUECO-POST |
| `PublishTransform` | NO TIENE | Sin sincronización explícita inmediata expuesta (el transform se publica al cierre del paso). | HUECO-POST |
| `SweepTest` | PARCIAL | `Physics.SphereCast` barre una esfera sobre el mundo, pero no barre los colliders del propio `RigidBody`. | HUECO-POST |
| `SweepTestAll` | NO TIENE | Sin barrido del propio cuerpo con todos los impactos. | HUECO-POST |
| `GetPointVelocity` | NO TIENE | No existe el método; su parte lineal se deriva de `GetLinearVelocity`, pero la angular no es legible desde C#. | HUECO-POST |
| `GetRelativePointVelocity` | NO TIENE | Ídem en espacio local. | HUECO-POST |
| `ClosestPointOnBounds` | NO TIENE | Sin consulta sobre el AABB del cuerpo. | HUECO-POST |
| `GetAccumulatedForce` | NO TIENE | Sin consulta de fuerza acumulada. | HUECO-POST |
| `GetAccumulatedTorque` | NO TIENE | Sin consulta de torque acumulado. | HUECO-POST |
| `ResetCenterOfMass` | NO TIENE | No hay centro de masa manual que restablecer. | HUECO-POST |
| `ResetInertiaTensor` | NO TIENE | Ídem para la inercia. | HUECO-POST |
| `Sleep()` | NO TIENE | Rapier duerme automáticamente; sin orden manual. | HUECO-POST |
| `WakeUp()` | NO TIENE | Sin método explícito (las interacciones despiertan internamente). | HUECO-POST |
| `IsSleeping()` | NO TIENE | Estado de reposo no expuesto. | HUECO-POST |
| `OnCollisionEnter` | TIENE | `GlassBehaviour.OnCollisionEnter`, desde eventos de contacto ordenados de Rapier vía `RuntimeBridge`. | — |
| `OnCollisionStay` | TIENE | `GlassBehaviour.OnCollisionStay`. | — |
| `OnCollisionExit` | TIENE | `GlassBehaviour.OnCollisionExit`. | — |
| `OnTriggerEnter` | TIENE | `GlassBehaviour.OnTriggerEnter`; sensores con `is_trigger`. | — |
| `OnTriggerStay` | TIENE | `GlassBehaviour.OnTriggerStay`. | — |
| `OnTriggerExit` | TIENE | `GlassBehaviour.OnTriggerExit`. | — |
| Carga `Collision` / `ContactPoint` | PARCIAL | `GlassEngine.Core/Collision.cs` solo lleva `gameObject` (la otra entidad) e `isTrigger`: sin `relativeVelocity`, `impulse`, `collider`, `rigidbody`, contactos, punto, normal ni separación. | HUECO-BETA |

**TIENE 11 · PARCIAL 10 · NO TIENE 36 sobre 57.**
**Cobertura ponderada (TIENE + ½·PARCIAL): 16/57 ≈ 28 %.**

## CharacterController (25 ítems: 12 propias, 8 heredadas de Collider, 5 métodos/enums)

| Ítem Unity | Estado | Evidencia en GlassEnginer | Clase |
|---|---|---|---|
| `center` | NO TIENE | Ausente en el diseño v1 a propósito (commit del bloque CC); `Position` expone el centro solo en lectura. | DECISIÓN |
| `height` | TIENE | `Height` en `Physics/CharacterController.cs`. | — |
| `radius` | TIENE | `Radius`. | — |
| `slopeLimit` | TIENE | `SlopeLimit`; Rust lo aplica a `max_slope_climb_angle` y usa `slopeLimit + 5°` en `min_slope_slide_angle`. | — |
| `stepOffset` | TIENE | `StepOffset`; Rapier va con `autostep: None` y la capa propia resuelve el escalón (muestra adelantada + puerta de discontinuidad). Verificado en vivo: peldaño 0,35 con step 0,4; bloqueo con 0,05. | — |
| `skinWidth` | TIENE | `SkinWidth` → `offset` del `KinematicCharacterController`. | — |
| `minMoveDistance` | NO TIENE | Sin umbral configurable de descarte. | HUECO-POST |
| `detectCollisions` | NO TIENE | La cápsula SÍ tiene collider real en el mundo (otros cuerpos chocan con ella), pero no hay interruptor para desactivarlo. | HUECO-POST |
| `enableOverlapRecovery` | NO TIENE | Sin interruptor ni garantía API de auto-expulsión (la despenetración de `move_shape` actúa, pero no es superficie). | HUECO-POST |
| `isGrounded` | TIENE | `IsGrounded` tras cada `Move`; `RefreshInfo` lo releyendo del mundo. | — |
| `velocity` | PARCIAL | Sin propiedad; el script la deriva del desplazamiento aplicado que devuelve `Move` dividido por `dt`. | HUECO-BETA |
| `collisionFlags` | NO TIENE | `Move` devuelve el `Vector3` aplicado, no flags `Sides`/`Above`/`Below`. | HUECO-BETA |
| `enabled` (heredada) | NO TIENE | El `CharacterController` C# no es un `Collider` Unity-like; solo `Create`/`Destroy`. | HUECO-POST |
| `bounds` (heredada) | NO TIENE | Sin AABB de la cápsula expuesto. | HUECO-POST |
| `excludeLayers` (heredada) | NO TIENE | Sin filtrado por instancia en el controlador C# (el collider interno usa el filtro por defecto). | HUECO-POST |
| `includeLayers` (heredada) | NO TIENE | Ídem. | HUECO-POST |
| `material`/`sharedMaterial` (heredados) | NO TIENE | Sin material físico asociado en la superficie del controlador (en Unity tampoco afecta a su deslizamiento). | HUECO-POST |
| `Raycast` (heredado) | PARCIAL | Existe `Physics.Raycast` sobre el mundo, no contra el collider del personaje. | HUECO-POST |
| `ClosestPoint` (heredado) | NO TIENE | Sin consulta geométrica sobre la cápsula. | HUECO-POST |
| `ClosestPointOnBounds` (heredado) | NO TIENE | Ídem sobre su AABB. | HUECO-POST |
| `Move` | PARCIAL | `Move(motion, dt)` recibe desplazamiento y devuelve el aplicado. Sweep & slide (`slide = true`), separación horizontal/vertical con los pies en el suelo, escalones, pendientes y snap propios; no devuelve `CollisionFlags`. | HUECO-BETA |
| `SimpleMove` | NO TIENE | Ausente en el diseño v1 a propósito; la gravedad la compone el script en el `motion`, como con `Move` en Unity. | DECISIÓN |
| `OnControllerColliderHit` | NO TIENE | En `character.rs`, `move_shape` recibe la callback como `|_| {}`: los `CharacterCollision` se descartan. Sin hits no hay empuje scriptable de dinámicos. | HUECO-BETA |
| `CollisionFlags` (enum) | NO TIENE | No existe el tipo; `IsGrounded` cubre solo la parte `Below`. | HUECO-BETA |
| `ControllerColliderHit` (clase) | NO TIENE | Sin collider/rigidbody golpeado, punto, normal, dirección ni longitud del impacto. | HUECO-BETA |

**TIENE 6 · PARCIAL 3 · NO TIENE 16 sobre 25.**
**Cobertura ponderada: 7,5/25 = 30 %.**
Extras sin equivalente Unity (no computan): `SnapDistance` (snap
configurable, en Unity es interno) e `IsSliding`.

## Huecos de beta/demo, con coste estimado

Rigidbody:
- **`mass` cableada** — coste BAJO: transportar `PhysicsData.mass` en
  `SpawnedBodySpec` y aplicarla como densidad por collider (o masa
  adicional) + propiedad C# en la creación por script. Hoy el campo
  MIENTE en el Inspector: es el hueco más urgente por engañoso.
- **`linearDamping` configurable** — BAJO: setter FFI + propiedad C# +
  campo de escena; el valor fijo 1.5 frena proyectiles y cuerpos que
  deberían conservar velocidad.
- **`useGravity` por cuerpo** — BAJO: `gravity_scale` de Rapier tras
  una FFI y una propiedad.
- **`ForceMode` en ApplyForce/ApplyImpulse** — BAJO: enum y rama en
  los wrappers (Acceleration = fuerza sin masa; VelocityChange =
  fijar velocidad).
- **`collisionDetectionMode` (CCD por cuerpo)** — BAJO: `ccd_enabled`
  al crear + setter; necesario si la demo sube la velocidad de
  proyectil (hoy el solver CCD existe pero ningún cuerpo lo activa).
- **`isKinematic` mutable** — BAJO: `set_body_type` de Rapier tras FFI.
- **Carga `Collision` con contactos** — MEDIO: los eventos ya fluyen
  ordenados; hay que enriquecer el evento (punto, normal, impulso,
  velocidad relativa) a través de FFI y del struct C#.
- **`MovePosition` cinemático** — MEDIO: mover por velocidad implícita
  en el paso (plataformas que empujan dinámicos sin teletransporte).
- **`interpolation`** — MEDIO-ALTO: par de transforms previo/actual y
  mezcla en el render; se nota en pantallas de más de 60 Hz.

CharacterController:
- **`velocity` de solo lectura** — BAJO: el aplicado ya se calcula en
  `Move`; exponer `aplicado/dt` como propiedad.
- **`CollisionFlags` + `OnControllerColliderHit`/`ControllerColliderHit`**
  — MEDIO: dejar de descartar los `CharacterCollision` de
  `move_shape`, derivar los flags (Below ≈ grounded; Sides/Above por
  normales) y despachar el hit a C# como los eventos de contacto. Es
  la pieza que habilita empujar cajas con el cuerpo y cortar el salto
  al golpearse la cabeza.

## Notas

- **Filosofía de lectura (suya, 2026-11-13):** cuando él describe una
  mecánica de Unity con su truco o su fallo, señala DÓNDE ocurre el
  problema, no prescribe la solución: la exigencia es que ese modo de
  fallo no exista aquí. Aplicado a las trampas del documento:
- **Trampa 3.4.1 (dos `Move` por frame):** el riesgo existe si el
  script llama dos veces; el estado refleja la última llamada. La
  separación horizontal/vertical interna NO es el problema: es un
  solo `Move` por llamada de script. Sin cambio de diseño.
- **Trampa 3.4.2 (isGrounded parpadeando en bajadas):** mitigada por
  diseño con `SnapDistance`/`snap_to_ground` (la cápsula se pega al
  suelo hasta 0,3 m), no dejada al truco de «velocidad vertical −2»
  del script. El modo de fallo queda reducido a pendientes que
  superen el snap.
- **Trampa 3.4.3 (plataformas móviles):** el controlador NO hereda
  hoy el movimiento de la plataforma: ese es exactamente el modo de
  fallo señalado. Queda registrado como requisito de diseño para
  cuando la demo lo necesite (la solución es nuestra: heredar el
  delta de la superficie bajo los pies, no el emparentado manual de
  Unity).
- **Teletransporte:** donde Unity obliga a desactivar el controlador
  un frame, aquí `Teleport` es operación de primera clase del mundo
  (verificada en el bloque CC): ese modo de fallo no existe.
- **Sin `FixedUpdate`:** los scripts corren por frame de render con
  `deltaTime`; la física avanza a paso fijo 1/60 con substeps (tope 4
  y deuda descartada, auditoría C4). La paridad con Unity aquí es
  parcial por arquitectura, no por ítem.
- **Lo PARCIAL por superficie, no por motor:** Rapier ya sabe hacer
  varias cosas que la tabla marca ausentes (torque, inercia,
  damping angular, CCD): el hueco está en el transporte FFI → C#,
  no en el solver. Por eso casi todos los costes son BAJO/MEDIO.

## Veredicto

- **Rigidbody: 28 % ponderado** (11 TIENE · 10 PARCIAL · 36 NO TIENE).
  El núcleo de uso diario está (crear, colliders, impulso/fuerza,
  velocidad lineal, transform, callbacks completos, restricciones
  por ejes desde hoy); lo que falta es sobre todo SUPERFICIE
  (parámetros que Rapier ya tiene) y dos piezas de comportamiento:
  masa real e interpolación.
- **CharacterController: 30 % ponderado** (6 TIENE · 3 PARCIAL ·
  16 NO TIENE). Lo esencial de producción está y está verificado en
  vivo (mover, escalón, pendiente, snap, suelo); le faltan los OJOS:
  flags y hits de colisión, y la velocidad legible.
- Ningún hueco es de motor: todos son de transporte o de diseño
  v1 reversible. Qué entra para la beta lo decide D3m3tr10 con esta
  tabla; esta auditoría no implementa nada.

Consumo de la auditoría: Kimi 14.682 (borrador de cruce) · la
verificación contra el código y la redacción final son de Rayo.
El desbloqueo asociado (`ede6bd1`): Sonnet 5, 12.490.
