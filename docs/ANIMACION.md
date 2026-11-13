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

Fuera de la beta (Post-1.0): blend trees 2D/Direct, capas,
AvatarMask, IK, root motion, StateMachineBehaviours,
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

**Auto-rigging** (último de la cola): requiere antes una vista de
huesos en el editor (hoy no existe). El rig de manos y dedos NO va
en el auto-rig principal: será un **auto-rig secundario y
opcional**, un módulo aparte que el desarrollador usa solo si lo
quiere. Detalle en `LISTA_DE_DESPUES.md`.
