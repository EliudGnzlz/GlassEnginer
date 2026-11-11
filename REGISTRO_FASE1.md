# REGISTRO — Fase 1: física completa (capas, consultas, eventos de contacto)

Cerrada el 2026-11-11. Plan: `ANALISIS_CONSOLIDADO.md` (Fase 1 + ampliación de
eventos de contacto a C#). Diseño e inspección: `fase1/DISENO_FASE1.md`,
`fase1/INSPECCION_FASE1.md` (fuera del repo, en `~/workspace/glass-engine/fase1/`).

## Entregado

### Datos y colliders (glass_core + glass_physics)
- `PhysicsData` gana `layer: u8` (0), `collision_mask: u32` (todos los bits) e
  `is_trigger: bool` (false), todos con `#[serde(default)]`: las escenas
  anteriores cargan sin cambios.
- `ColliderConfig { layer, mask, is_trigger }` en `glass_physics::types`; los
  attach de caja/esfera/cápsula/cilindro aceptan config (las firmas históricas
  quedan como wrappers con la config por defecto). Grupos Rapier:
  memberships = bit de la capa, filter = máscara; todo collider lleva
  `ActiveEvents::COLLISION_EVENTS` y `.sensor(is_trigger)`.
- `attach_convex_collider`: casco convexo de Rapier expuesto (mesh collider
  convexo de la caja de herramientas; la parte compuesta/trimesh es la
  Fase 1.5, decisión del usuario 2026-11-11, opción B).

### Eventos de contacto (glass_physics)
- El paso usa `ChannelEventCollector` de Rapier 0.23 (crossbeam) en lugar de
  `&()`: eventos Started/Stopped → `ContactEvent { a, b, kind: Enter|Exit,
  is_trigger, step }` con el par ordenado. Stay no es evento del mundo: lo
  deriva el consumidor de los pares activos.
- API: `drain_contact_events()`, `contact_pairs()`, `recent_contact_events(n)`
  (anillo de 256), `step_count()`.

### Consultas (glass_physics)
- `cast_ray_masked` (el `cast_ray` histórico delega con máscara completa),
  `cast_sphere`, `cast_capsule`, `overlap_sphere`, `overlap_box`. Semántica:
  la consulta ve los colliders cuya capa esté en la máscara dada; todas
  aceptan entidad a excluir.

### Puente a C#
- `Collision` (struct, namespace GlassEngine.Core): `gameObject` = la OTRA
  entidad, resuelta con su nombre real de escena (el host lo pasa por la FFI
  desde esta fase: sin esto el wrapper nacía como «Entity»), e `isTrigger`.
- Virtuales en `GlassBehaviour`: OnCollisionEnter/Stay/Exit y
  OnTriggerEnter/Stay/Exit.
- Export gestionado `DispatchContactEvent` (resolución tolerante en el host,
  como el resto de puentes) y despacho en la app tras el paso de física, con
  el lock ya soltado, a ambas entidades del par; Stay = pares que ya estaban
  activos el frame anterior y no acaban de entrar.

### MCP (20 → 22 tools)
- `glass_physics_cast` (kind: ray | sphere | capsule | overlap_sphere |
  overlap_box; origin/direction/radius/half_height/half_extents/max_dist/
  mask/exclude_entity).
- `glass_list_contact_events` (active_pairs, recent_events, step_count).

### Contenido y documentación
- `scripts/gen_escenas_estres.py` → `assets/scenes/StressTest100/500/1000.json`
  (rejilla de esferas/cajas dinámicas sobre suelo, alturas escalonadas).
- `assets/scenes/Fase1EventosDemo.scene.json` + script
  `managed/GlassEngine.Core/Scripts/EventosDemo.cs`.
- `docs/PHYSICS.md`: modelo de datos, capas/máscaras, triggers, consultas,
  eventos, estrés y límites conocidos.

## Verificación

### Unitaria
- 8 tests nuevos del mundo: capas cruzadas que se ignoran entre sí y colisionan
  con el suelo; trigger que no bloquea y genera Enter/Exit; Enter de colisión
  normal + par activo; Exit al teletransportar; SphereCast con normal +Y y
  distancia; rayo con máscara por capas; overlap que lista solo lo de dentro;
  hull convexo que sostiene como una caja.
- 1 test MCP nuevo (tools/list anuncia las 2 tools; cast sin mundo → hit null;
  estructura de contactos).
- **Suite completa del workspace: 279 tests, 0 fallos** (270 al cierre de la
  Fase 0), con `RUSTFLAGS="-D warnings"`. Build gestionado C#: 0 errores.

### Test físico en la VM (lavapipe, 1,5 CPU — contexto, no medida limpia)
- Demo de eventos (escena de 6 entidades, log del script C#):
  `CollisionEnter con Suelo` ×15, `TriggerEnter con Sensor` ×2,
  `TriggerExit con Sensor` ×2 — nombres reales tras el arreglo de la FFI.
- Estrés 1000 cuerpos (1001 cuerpos reconstruidos): `glass_frame_timing` con
  112 muestras — física avg 124 ms / p95 189 ms por frame (hasta 4 substeps
  por frame bajo render por software; total de frame ~1,5 s: el coste medido
  es el de la VM, no el del motor en hardware real).
- Casts en vivo sobre la escena de estrés: rayo desde (0,30,0) hacia abajo →
  suelo (entidad 3), distancia 30,0, punto (0,0,0), normal (0,1,0);
  overlap_sphere en el centro → 51 entidades.
- Pares activos en vivo (StressTest100): 80 pares suelo-cuerpo en el paso 198.
  Nota: las consultas hechas demasiado pronto (paso ~114) devolvían 0 pares
  porque los cuerpos aún no habían aterrizado — la caída frenada del motor
  (amortiguación 1,5) hace que la primera fila toque el suelo hacia el paso
  ~110 en estas escenas; no es un defecto de `contact_pairs` (verificado con
  instrumentación temporal, ya retirada).
- Capturas en `fase1/test-fisico/` (fuera del repo).

## Incidencias de la fase (honestidad de registro)
- La primera integración del export gestionado quedó empalmada dentro del
  catch de otro método de RuntimeBridge.cs (la compilación gestionada lo
  cazó); se reubicó como método de clase y el build quedó en 0 errores.
- DeepSeek quemó 3 paquetes de consultas por razonamiento sin contenido
  (41.570 tokens); el subpaquete se reasignó a Claude tras verificar la API
  de Rapier 0.23 en el código vendored. Claude supuso en dos sitios APIs de
  versiones anteriores (`ChannelEventHandler`, `QueryPipeline::update` con
  dos argumentos): corregidas contra rapier3d 0.23.1/parry 0.18 reales.
- La VM fue reemplazada dos veces durante la fase; los paquetes de Vulkan
  (mesa-vulkan-drivers, libxkbcommon-x11-0, glslang-tools, xdotool) se
  reinstalaron antes del test físico.

## Consumo por proveedor (tokens)
- DeepSeek: 103.013 (DS-1 18.360 y DS-1b/DS-1b1 23.210, fallidos;
  DS-1a 8.977; DS-2 10.389; DS-3 16.008; DS-3b 13.091; DS-3c 12.978, fallido).
- Claude: 41.058 (CL-1a 10.591; CL-1c 12.980; CL-2 11.243; CL-3 6.244;
  CL-1 original: timeout del helper, sin usage fiable).
- Kimi: 9.804 (KM-1 5.369: generador de estrés + escena demo + script;
  KM-2 4.435: docs/PHYSICS.md).

## Deuda / siguiente
- **Fase 1.5 — caja de herramientas de colliders** (decisión 2026-11-11):
  múltiples colliders primitivos por entidad en el modelo de escena y
  trimesh estático. El mundo ya admite varios colliders por cuerpo por
  código; falta el modelo de datos con offsets y el camino de vértices.
- Sin superficie `UnityEngine.Physics` en C# todavía: las consultas viven en
  Rust y en MCP (registrado en docs/PHYSICS.md como límite conocido).
- Observación de la Fase 0 que sigue vigente: bajo carga extrema alguna
  llamada MCP por stdio tarda en responder (en esta fase, con la escena de
  1000 cuerpos a ~1,5 s/frame, las llamadas posteriores a la primera
  necesitaron reintento con esperas de 60 s y orden distinto).
