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

## Addenda 2026-11-13 — Criterios «mejor que Unity» del CharacterController

Orden suya (14:10): dos modos de fallo de Unity señalados por él como
previsión de diseño — en la lectura correcta de sus descripciones
(precisión de las 14:13), él señala DÓNDE ocurre el problema, no
cómo debe construirse la solución; la exigencia es que el fallo no
exista aquí ni en las mecánicas similares. Estado tras implementarlos
y verificarlos:

| Modo de fallo de Unity a prevenir | En Unity | En GlassEnginer | Verificación |
|---|---|---|---|
| Teletransporte del CharacterController | Hay que desactivar el componente un frame (`enabled=false`, mover, `enabled=true`) porque el estado de colisión del barrido pelea con el cambio de posición | **Nativo**: `Teleport` es operación de primera clase (fija la posición y limpia el estado en el acto, incluido el suelo recordado: sin empujes ni arrastres residuales al llegar) | Test Rust `teletransporte_limpia_el_arrastre` + fase en vivo `teletransporte_limpio` (x constante ±0,01 tras saltar de un ascensor en marcha al suelo) |
| Plataformas móviles y ascensores | El personaje NO hereda el movimiento de la plataforma: hay que programar el arrastre a mano en cada script | **Nativo**: el controlador recuerda su suelo (sonda tras cada Move) y aplica el delta del cuerpo suelo antes de su propio deseo, en horizontal y en vertical; bajar caminando se suelta solo al cambiar el suelo bajo los pies | Tests Rust `plataforma_horizontal_arrastra`, `ascensor_sube_y_baja_sin_despegarse`, `bajar_caminando_suelta_la_plataforma` + escena en vivo `PruebaPlataformasCC` |

La misma previsión ya aplicada en mecánicas parecidas: el
`isGrounded` que parpadea al bajar (§3.4) se previene por diseño
con el snap + la sonda de suelo (el suelo recordado solo se suelta
al perder el contacto real), y el teletransporte limpio cubre
también los respawns y portales que recolocan al personaje (el
cruce de portal de la Fase 9 conserva el impulso por la misma vía).

Dos hallazgos de la implementación, por honestidad técnica:
(1) las posiciones de los colliders de Rapier solo se propagan
desde sus cuerpos al hacer `step()`, y el personaje se mueve desde
los scripts ANTES del step: `move_character` sincroniza ahora los
colliders del cuerpo suelo antes de consultar, o la geometría de
la plataforma iría siempre un paso por detrás del arrastre.
(2) La fricción cinemática interna de Rapier 0.23 (sin interruptor
en esta versión) anula el empuje normal con el que `move_shape` se
separa de la superficie, y el barrido horizontal sobre un cuerpo
cinemático se auto-bloqueaba en un bucle de impactos a toi 0
(verificado: la misma pasada mueve sobre Fixed y devuelve
[0,0,0] sobre Kinematic): la pasada horizontal separada sale una
piel por encima cuando el suelo es cinemático y la vertical
posterior asienta al personaje. El arrastre cubre la TRASLACIÓN
del suelo; una plataforma que ROTA bajo los pies queda fuera de
esta versión (el CharacterController no rota por diseño).

---

## Addenda 2026-11-13 (noche) — Bloque de huecos beta IMPLEMENTADO

Orden del usuario (15:31): implementar los huecos clasificados
para la beta. Estado tras el bloque (commits `c0b3053`, `a0aa31d`,
`471fe0d`, `e8c0b5f`, `0ad38db`, `924af33`):

**Rigidbody — de 11 TIENE · 10 PARCIAL · 36 NO TIENE (≈28 %) a:**

- `mass` **cableada de verdad**: mundo (reescalado de densidades),
  FFI, escena (las DOS rutas: spawn en vivo y rebuild) y C#
  `RigidBody.Mass` (se re-aplica al adjuntar colliders, porque la
  masa manda sobre la densidad y los colliders llegan después).
- `linearDamping` configurable: `PhysicsData.linear_damping`
  (defecto 1.5, el histórico) y `RigidBody.LinearDamping`. El
  1.5 fijo dejó de ser invisible.
- `useGravity` por cuerpo: `PhysicsData.use_gravity` +
  `RigidBody.UseGravity`/`GravityScale` (gravity_scale de Rapier).
- `ForceMode` completo en `ApplyForce`/`ApplyImpulse`:
  Force, Impulse, Acceleration (f × masa) y VelocityChange
  (Δv directo, verificado en vivo: v = 3,00 exactos con masa 5).
- `collisionDetectionMode`: `PhysicsData.ccd` +
  `RigidBody.CollisionDetection`; verificado en vivo (proyectil
  a 80 m/s frenado por CCD; su gemelo discreto tunela la pared).
- `isKinematic` mutable en caliente (`set_body_type`: Dynamic /
  Static / KinematicPositionBased) + `MovePosition` cinemático
  que empuja dinámicos (test de superficie).
- Carga `Collision` **con contactos**: punto (media de los puntos
  del manifold), normal orientada por dueño del collider
  (semántica Unity; el orden de creación ya no la invierte),
  velocidad relativa por receptor e impulso — el del manifold
  llega a 0 en el Enter, así que si es cero se estima como masa
  reducida × velocidad de aproximación (verificado en vivo:
  2 kg a 4,73 m/s → 9,46).
- `interpolation`: captura por substep del transform previo,
  mezcla por resto del acumulador con slerp y regla de salto
  (>2 m no se barre). `RigidBody.Interpolate` por entidad.

Recuento: TIENE **19** · PARCIAL **10** · NO TIENE **28** de 57
(≈42 % ponderado). Siguen fuera, Post-1.0 según la clasificación:
drag angular y tensor de inercia, solverIterations y familia,
maxDepenetrationVelocity, centerOfMass automático, Sleep/wake
con umbrales configurables, `AddExplosionForce` como API
directa (el patrón compuesto de la Fase 10 sigue siendo la vía),
consultas de barrido del Rigidbody (sweep test) y la familia de
`ComputePenetration` fina.

**CharacterController — de 6/3/16 (30 %) a:**

- `velocity` legible: `CharacterController.Velocity` (desplazamiento
  aplicado ÷ dt del último Move).
- `CollisionFlags` + `OnControllerColliderHit`: las
  `CharacterCollision` ya no se descartan; se convierten en
  `ControllerColliderHit` (entidad, punto witness2, normal1,
  dirección y longitud) con flags Below/Sides/Above por la
  normal (verificado en vivo contra la pared: normal −X,
  entidad correcta, flags Below|Sides). La corrección de API la
  detectó el paquete: en Rapier 0.23 el impacto es
  `hit: ShapeCastHit` de parry, no `toi: TOI`.

Recuento: TIENE **9** · PARCIAL **3** · NO TIENE **13** de 25
(42 %). Quedan fuera (Post-1.0): `minMoveDistance`,
`detectCollisions` desactivable, stepOffset/slopeLimit como
propiedades mutables tras crear (hoy van en la configuración de
creación), y la familia heredada de Collider (material físico
por personaje y bounds).

**Verificación del bloque:** tests de mundo — superficie RB 6/6,
contactos 4/4, hits del CC 3/3, interpolación 3/3, sin regresión
en escalones/plataformas del CC — y **prueba en vivo
`PruebaHuecosBeta` GLOBAL OK** (escena y scripts en el repo).

---

## Addenda 2026-11-14 — Bloque de PARIDAD (altas y medias) IMPLEMENTADO

Orden del usuario (2026-11-13 22:56): las piezas ALTA y MEDIA
que la addenda anterior dejó fuera. Las BAJAS quedan registradas
en `LISTA_DE_DESPUES.md` §7, no descartadas.

**Rigidbody — nuevo en la superficie (mundo + FFI + C#):**

- `angularVelocity` legible y fijable (`RigidBody.AngularVelocity`).
- `AddForceAtPosition` con los cuatro `ForceMode` (el torque sale
  de r×F sobre el centro de masa real).
- `AddTorque` y `AddRelativeTorque` (local→mundo con la rotación
  actual; en Acceleration/VelocityChange escalan por masa —
  aproximación documentada, sin tensor).
- `AddExplosionForce(force, pos, radius, upwardsModifier, mode)`
  como API: dirección desde el COM con el centro efectivo
  desplazado −upwards en Y, factor lineal 1−dist/radio.
- `detectCollisions` por cuerpo: conserva la membresía de capa y
  pone el filtro a NONE; al reactivar restaura los grupos
  originales.
- `centerOfMass` / `worldCenterOfMass` / `ResetCenterOfMass` y
  `inertiaTensor` / `inertiaTensorRotation` / `ResetInertiaTensor`.
  Diseño del override: `MassProperties` de parry suma ponderando
  por masa, así que «deseado − actual» con igual masa da una
  adicional de masa cero que no mueve nada; la implementación
  reduce las densidades a la mitad y compone la adicional contra
  esa mitad (masa M/2 > 0), con componentes COM e inercia
  separados (fijar solo el COM no congela la inercia: tras
  `set_mass` la parte no fijada escala con la masa). Caveat:
  añadir o quitar colliders tras un override exige volver a
  fijar COM/inercia.
- `GetPointVelocity` / `GetRelativePointVelocity` (v + ω×(p−COM)).
- `Move(pos, rot)` y `MoveRotation(q)` cinemáticos (next position
  completa de Rapier).
- `SweepTest` / `SweepTestAll` del propio cuerpo: el barrido de
  Rapier solo devuelve el primer impacto, así que el «All» itera
  excluyendo el cuerpo impactado (tope 64) y ordena por
  distancia. El FFI del «All» transporta entidad + distancia; el
  punto y la normal quedan a cero (documentado en el envoltorio).
- `maxLinearVelocity` / `maxAngularVelocity`: topes aplicados
  DESPUÉS de cada substep (antes del step, la gravedad del propio
  substep dejaba la velocidad en tope+0,16).
- `Sleep()` / `WakeUp()` / `IsSleeping()` + `SleepThreshold`
  (defecto Unity 0,005; mapeo a la energía de activación de
  Rapier; umbral ≤ 0 = nunca duerme).

**CharacterController — nuevo:**

- `DetectCollisions`: con la cápsula ya invisible al solver
  (arreglo de la demo), su semántica es el modo fantasma de las
  sondas de movimiento: false = `Move` atraviesa todo.
- `EnableOverlapRecovery` como superficie real: pasada de
  expulsión por contactos de parry. Ojo verificado contra el
  fuente de parry 0.18: `Contact.normal1` apunta al exterior de
  la PRIMERA forma; la expulsión de la cápsula usa `normal2`.
- `StepOffset` / `SlopeLimit` mutables tras crear (empujan al
  motor en caliente), `Enabled` (Move no aplica nada), `Bounds`
  (AABB de la cápsula) e `IncludeLayers` (máscara de capas de
  todas las sondas; la semántica exacta de `excludeLayers` de
  Unity sigue fuera, en las bajas).
- Arreglo asociado: el `applied` de `Move` no incluía el empuje
  de la recuperación de solape; ahora es posición final −
  posición de entrada, siempre.

**Recuento fresco sobre la tabla de ítems** (sustituye la
aritmética de la addenda anterior, que mezclaba gradaciones):

- **Rigidbody: TIENE 43 · PARCIAL 5 · NO TIENE 9 de 57 →
  ponderado 45,5/57 ≈ 80 %.** PARCIAL: `automaticCenterOfMass`,
  `automaticInertiaTensor` (equivalen a «siempre automático»),
  `excludeLayers`, `includeLayers` (matriz de capas sin la
  semántica aditiva exacta de Unity) y la carga `Collision`
  (completa en datos; sin referencias `collider`/`rigidbody`).
  NO TIENE: `angularDamping`, `maxDepenetrationVelocity`,
  `solverIterations`, `solverVelocityIterations`,
  `AddRelativeForce`, `PublishTransform`, `ClosestPointOnBounds`,
  `GetAccumulatedForce`, `GetAccumulatedTorque` — las nueve
  están en las BAJAS registradas (§7).
- **CharacterController: TIENE 16 · PARCIAL 2 · NO TIENE 7 de
  25 → ponderado 17/25 = 68 %.** PARCIAL: `Move` (sin
  `CollisionFlags` como valor de retorno; los flags van por
  `LastFlags`) y el `Raycast` heredado (sobre el mundo, no
  contra la cápsula). NO TIENE: `center` y `SimpleMove` (por
  decisión de diseño v1), `minMoveDistance`, `excludeLayers`,
  `material`/`sharedMaterial`, `ClosestPoint`,
  `ClosestPointOnBounds`.

**Verificación:** tests Rust nuevos `paridad_a` 7/7, `paridad_b`
8/8, `paridad_c` 10/10 sin regresión en la batería del CC
(plataformas 4/4, escalones 4/4, hits 3/3, cápsula invisible
2/2), y **prueba en vivo `PruebaParidad` GLOBAL OK: 20/20**
chequeos contra la superficie C# (torque ω=3,00 exactos; fuerza
en punto vz=4,00 y ωy=−12,00; explosión (4,93, 0,82); sweep a
4,00 que pasa a 6,00 tras el `Move` cinemático; CC bloqueado a
5,13 exactos = cara − radio − piel; subida del peldaño con pico
0,35). Primera corrida: 19/20 — el fallo era del veredicto de
mi test (medía la subida NETA y el personaje bajaba por el otro
lado del peldaño dentro de la ventana: neto ≈ 0 habiendo subido
bien); corregido a pico, 20/20. El motor no se tocó por ese
fallo.

**Consumo del bloque: Sonnet 224.271** (PAQ-A 23.654 + PAQ-B2
85.759 + PAQ-C 58.344 + PAQ-D 47.807 productivos; + 8.707 del
primer intento del PAQ-B, que llegó sin código por pedir los
archivos completos — reenviado con ellos pegados). **Opus 0
productivos:** la pasarela devolvía 200 con cuerpo vacío para
`claude-opus-5-5` (sonda mínima incluida; Sonnet por la misma
pasarela, normal) — caída externa diagnosticada el 2026-11-14,
no arreglable desde aquí; ~3–6k tokens de prompt quemados en
las sondas. Según la regla de fallos de API, el bloque se movió
a Sonnet con la evidencia, y Opus se reintentará en el siguiente
bloque. DeepSeek 0 · Kimi 0. Los tres defectos de integración
(álgebra de masa cero del override, `sweep_all` de un solo
impacto, signo de la normal de expulsión) los cazaron los tests
antes de la prueba en vivo y se arreglaron a mano.

## Addenda 2026-11-16 — Bloque RB/CC 100% (§7) IMPLEMENTADO + re-auditoría

Implementadas TODAS las piezas BAJA del §7 de `LISTA_DE_DESPUES.md`
(registro completo en `REGISTRO_RBCC100.md`): RB-1 `angularDamping` +
`solverIterations`; RB-2a `AddRelativeForce` +
`GetAccumulatedForce`/`GetAccumulatedTorque`; RB-2b
`maxDepenetrationVelocity` (límite real sobre el desplazamiento de
separación por substep: la separación de Rapier es posicional, no por
velocidad); RB-3 semántica Unity de `includeLayers`/`excludeLayers` +
`solverVelocityIterations` (valor guardado, manda el máximo de ambas);
RB-4 toggles `automaticCenterOfMass`/`automaticInertiaTensor` +
`PublishTransform` + `ClosestPointOnBounds`; CC-1 `minMoveDistance` +
`center` + `SimpleMove`; CC-2 `material`/`sharedMaterial` (clase
`PhysicMaterial` mínima creada: no existía en ninguna capa) +
`Raycast`/`ClosestPoint`/`ClosestPointOnBounds` contra la propia
cápsula (analíticos con parry).

**Re-auditoría contra las tablas originales (82 ítems):**

- **Rigidbody: TIENE 56 · PARCIAL 1 · NO TIENE 0 de 57 → 99,1 %
  ponderado.** El único PARCIAL es la carga `Collision`/`ContactPoint`:
  lleva `point`, `normal`, `impulse` y `relativeVelocity`, pero no
  `contacts[]` ni referencias `collider`/`rigidbody`.
- **CharacterController: TIENE 24 · NO TIENE 1 de 25 → 96 %.** El
  único hueco es `excludeLayers` heredado de Collider (el CC sí tiene
  `includeLayers`); quedó FUERA del alcance ordenado en el §7.
- **Alcance ordenado (§7): 13/13 RB + 8/8 CC = 100 %.**

Adaptaciones documentadas (capacidad presente, forma distinta a
Unity): `AddForce` → `ApplyForce`/`ApplyImpulse` con `ForceMode`
(los 4 modos); `linearVelocity` → `Get/SetLinearVelocity`;
`Move` del CC devuelve el desplazamiento aplicado y expone
`LastFlags` (`CollisionFlags`); `Raycast` del CC usa
origin/direction (el motor no tiene tipo `Ray`);
`GetAccumulatedForce` es la suma viva (Rapier no la resetea por
paso); el material del CC queda aplicado a su collider, pero la
cápsula es invisible al solver (GROUP_32/NONE), así que hoy no
cambia interacciones con otros cuerpos.

**La sonda física destapó 3 defectos reales en código ya entregado**
(verificados y arreglados de raíz; detalle en `REGISTRO_RBCC100.md`):
(1) `PublishTransform` (RB-4) tenía la condición de éxito invertida
— nunca publicaba en cuerpos de escena y escribía el transform por
defecto (cuaternión cero) en cuerpos de script; (2) normalizar un
cuaternión cero producía NaN y envenenaba el cuerpo — guardas en
`set_transform` y `create_rigid_body`; (3) `glass_physics_set_transform`
devolvía `bool` Rust (1 byte) leído como BOOL C# (4 bytes): un
`false` se leía como `true` — convertido al patrón i32. Y la caché
`Position` del CC no seguía el desplazamiento de `center` (CC-1).
Sonda: escena `PruebaRbcc100` + script `Rbcc100Probe` — **29/29 OK**
en standalone tras los arreglos.

## Addenda CCX (2026-11-16, orden posterior al cierre): `excludeLayers` del CC — HECHO

La única pieza que quedó fuera del alcance del §7 se implementó el
mismo día por orden expresa de D3m3tr10, como espejo exacto de
`includeLayers` del CC y de las capas del Rigidbody de RB-3:

- `CharacterState.exclude_mask: u32` (defecto 0); `cc_filter` aplica
  la máscara efectiva `include & !exclude` en las tres sondas del
  controlador (despenetración, movimiento y consultas), con la
  semántica de Unity (exclude manda sobre include).
- FFI `glass_character_set/get_exclude_layers` (patrón i32), campos
  al final de `PhysicsBridgeCallbacks` en ambos lados del puente, y
  propiedad C# `CharacterController.ExcludeLayers` (int, defecto 0).
- Tests nuevos `ccx_*` (4) con movimiento real: aterriza en el suelo
  de una capa, lo atraviesa si la capa está excluida, y exclude manda
  sobre include en las tres combinaciones. Sonda física
  `PruebaRbcc100` extendida: **32/32 OK** (defecto, set/get y la
  caída a través del suelo con su capa excluida: y = −4,9 sin suelo).

**Recuento actualizado: CharacterController TIENE 25 · NO TIENE 0
de 25 → 100 %.** La fila `excludeLayers` (heredada) de la tabla del
CC pasa de NO TIENE a TIENE. El Rigidbody queda como estaba:
56/57 con 1 parcial → 99,1 % ponderado. Commit `a4a1a24`,
CI 43967247043 success. Consumo CCX: Opus 25.928.
