# Animación — estado del módulo

**Módulo CERRADO para la beta** (auditoría del 2026-11-12 contra el
código). Alcance de la beta:

- FSM con AnyState, exit_time y triggers (Fase 4).
- Blend tree 1D sobre la FSM, parámetro float `Speed`.
- Skinning GPU con clips glTF (incl. modelos de Mixamo).
- Tools MCP: `glass_animator_state`, `glass_animator_set_param`.

Fuera de la beta (Post-1.0): blend trees 2D/Direct, capas,
AvatarMask, IK, root motion, StateMachineBehaviours,
OverrideController, Playables, retargeting/Avatar, Blend Shapes (el
importador los salta a propósito) y edición de curvas/keyframes.

# Animación — estado del módulo

**Módulo CERRADO para la beta** (auditoría del 2026-11-12 contra el
código). Alcance de la beta:

- FSM con AnyState, exit_time y triggers (Fase 4).
- Blend tree 1D sobre la FSM, parámetro float `Speed`.
- Skinning GPU con clips glTF (incl. modelos de Mixamo).
- Tools MCP: `glass_animator_state`, `glass_animator_set_param`.
- **Eventos de animación** (integrados el 2026-11-13, ver abajo).
- **StateMachineBehaviours**: `OnStateEnter` / `OnStateExit` en C#
  (integrados el 2026-11-13, ver abajo).
- **Blend trees 2D** (integrados el 2026-11-13, ver abajo).
- **Root motion** (integrado el 2026-11-13, ver abajo).

Fuera de la beta (Post-1.0): blend tree Direct, capas,
AvatarMask, IK,
OverrideController, Playables, retargeting/Avatar, Blend Shapes (el
importador los salta a propósito) y edición de curvas/keyframes.

## Eventos de animación

Declarados en el JSON del controlador, en el estado (`events` de
`StateDef`, para estados de un solo clip) o en el hijo de blend tree
(`events` de `BlendChild`, el caso de los pasos al caminar):

```json
{ "name": "PasoIzq", "time": 0.25, "value": 1.0, "text": "izq" }
```

- `time` es normalizado 0..1 sobre la duración del clip; `value`
  (f32) y `text` son opcionales.
- El disparo se calcula en `AnimatorData::advance` (puro y testeado):
  cruce en tiempo absoluto, con envoltura en clips en bucle, una sola
  vez al llegar al clamp en clips sin bucle, y **como máximo una
  ocurrencia por evento y frame** aunque un dt gigante salte varios
  ciclos (regla documentada: a dt extremo se pierden pasos
  intermedios, no se inventan).
- Los hijos de blend solo disparan si su peso en el frame es > 0;
  el origen de un fundido sigue disparando mientras mezcla.
- Entrega a C#: el motor llama al export gestionado
  `DispatchAnimEvent` (resolución tolerante: un ensamblado sin el
  export arranca igual, sin eventos) y el behaviour de la entidad
  recibe `OnAnimationEvent(string name, float? value, string? text)`.
- Telemetría: `glass_animator_state` incluye `recent_events` (los
  últimos 10 de la entidad, con estado, clip, instante y datos).
- Prueba física: `PruebaEventosAnim.scene.json` con los controladores
  `eventos_test` (blend) y `eventos_simple` (estado simple) y el
  guión `EventosAnimTest.cs`: PasoIzq/PasoDer alternando por ciclo y
  PasoSimple por ciclo, verificados en el log y por MCP.

## StateMachineBehaviours

El behaviour de una entidad con Animator recibe los cambios de
estado de su máquina:

```csharp
public override void OnStateEnter(string state) { }
public override void OnStateExit(string state) { }
```

- La detección vive en `AnimatorData::advance`: se compara el
  estado a la entrada y a la salida del frame, lo que cubre
  transiciones por condición, peticiones `Play`/`CrossFade` y el
  **alta inicial** (el primer frame emite solo la entrada al
  estado por defecto, sin salida).
- En un fundido, la salida del origen y la entrada al destino se
  notifican **al arrancar el fundido**, no al completarse: un
  `CrossFade` de 0,5 s entrega ambos callbacks en el mismo frame.
  Un cambio intermedio dentro del mismo frame (fundido
  interrumpido) notifica el cambio neto del frame.
- `Play` al estado ya activo no emite nada (sin salida ni
  re-entrada), como en Unity.
- Entrega: export gestionado `DispatchStateEvent` con la misma
  resolución tolerante que los eventos; telemetría en
  `glass_animator_state` como `recent_state_events`.
- Prueba física: `PruebaEstadosAnim.scene.json` con
  `EstadoAnimTest.cs` sobre el controlador `xbot`: secuencia
  completa Idle → Walk → Idle → Walk (CrossFade) → Idle verificada
  en el log y por MCP, sin duplicados al completarse los fundidos.

## Blend trees 2D

Adelanto post-beta por orden de D3m3tr10 (2026-11-13). Un estado
puede declarar `blend2d` en vez de `blend` o `clip` (precedencia:
blend2d > blend > clip):

```json
{ "name": "Movimiento", "clip": "", "loop": true,
  "blend2d": {
    "param_x": "SpeedX", "param_y": "SpeedY",
    "children": [
      { "clip": "Idle", "position": [0, 0] },
      { "clip": "Walk", "position": [0, 2] },
      { "clip": "Run",  "position": [2, 0] }
    ] } }
```

- Cada hijo vive en una posición (x, y) del espacio de los dos
  parámetros float. Pesos por **distancia inversa al cuadrado**
  (Shepard) normalizada a suma 1; sobre la posición de un hijo,
  ese hijo pesa 1; pesos ≤ 1e-3 se podan y se renormaliza.
- Todo lo demás es lo mismo que en el 1D: relojes propios por
  hijo, eventos de animación en los hijos (solo con peso > 0),
  exit_time contra el hijo dominante y fundidos entre estados.
- `glass_animator_state` informa del árbol activo: con 1D trae
  `param`/`value`; con 2D, `param_x`/`param_y`/`value_x`/`value_y`
  y los hijos con su `position`, `weight` y `time`.
- El tipo «Direct» de Unity y el freeform direccional quedan
  fuera (sin demanda concreta).
- Prueba física: `PruebaBlend2D.scene.json` con el controlador
  `blend2d_test` (X Bot): pesos verificados por MCP en (0,0),
  (0,2), (1,1) — tercios exactos — y (2,0), y el evento
  `PasoBlend2D` del hijo Walk recibido. Registro:
  `REGISTRO_BLEND2D.md`.

**Auto-rigging** (último de la cola): requiere antes una vista de
huesos en el editor (hoy no existe). El rig de manos y dedos NO va
en el auto-rig principal: será un **auto-rig secundario y
opcional**, un módulo aparte que el desarrollador usa solo si lo
quiere. Detalle en `LISTA_DE_DESPUES.md`.

## Root motion

Adelanto post-beta por orden de D3m3tr10 (2026-11-13). Un estado
lo activa con `root_motion: true` en el JSON del controlador:

```json
{ "name": "Anda", "clip": "Walk", "loop": true, "root_motion": true }
```

- El desplazamiento se mide sobre el **nodo raíz** (la primera
  articulación cuyo padre no es articulación; en Mixamo, las
  caderas) en **espacio global del modelo**: la escala y la
  rotación de la armadura ya quedan aplicadas.
- Por frame, un seguidor (`RootMotionTracker`) compara la
  posición global de la raíz con la del frame anterior por cada
  clip muestreado (con desenvoltura de bucle) y mezcla los
  deltas por peso. El delta, rotado por la orientación de la
  entidad, se suma a su posición; si la entidad tiene cuerpo
  físico, el cuerpo se teletransporta con ella.
- La pose se evalúa con la raíz **fijada a su traslación de
  reposo**: el personaje no se escapa de su origen, se mueve la
  entidad.
- **v1 mueve solo en el plano XZ**: la Y del clip no se aplica
  (el Walk de Mixamo ni siquiera cierra el bucle en altura y el
  personaje se hundía caminando).
- `glass_animator_state` informa `root_motion` y la `position`
  actual de la entidad.
- Prueba física: `PruebaRootMotion.scene.json` con los
  controladores `rootmotion_test` (con el flag) y
  `rootmotion_control` (sin él): 46,62 m de avance recto frente
  a 0,0000 m del control.
