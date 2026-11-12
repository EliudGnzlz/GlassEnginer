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

**Eventos de animación**: único hueco barato candidato a colarse
antes de la beta — señalado en `LISTA_DE_DESPUES.md`, sin decisión
de implementarlos todavía.

**Auto-rigging** (último de la cola): requiere antes una vista de
huesos en el editor (hoy no existe). El rig de manos y dedos NO va
en el auto-rig principal: será un **auto-rig secundario y
opcional**, un módulo aparte que el desarrollador usa solo si lo
quiere. Detalle en `LISTA_DE_DESPUES.md`.
