# Registro — Undo/redo del editor (2026-11-13)

Ctrl+Z deshace y Ctrl+Shift+Z / Ctrl+Y rehace los cambios de
la escena hechos desde el editor, sobre la selección única
(vista 3D, Hierarchy o Inspector). Diseño en
`~/workspace/glass-engine/fase1/diseno_undo.md` (fuera del repo).

## Implementación

- **`crates/glass_editor/src/undo.rs`** (nuevo): historial por
  instantáneas `(entities, next_entity_id)` con huella FNV-1a
  del JSON para detectar cambios (`EntityData` no es
  `PartialEq`; no se tocó `glass_core`). `UndoHistory::observe`
  consolida un paso cuando el cambio lleva un frame estable y
  no hay edición en curso; tope de 64 pasos. 5 tests unitarios.
- **`docking.rs`**: campo `undo_history` en
  `EditorDockingLayout`; `apply_history` aplica el paso sobre
  el `SceneState` compartido y ajusta la selección (se
  conserva si la entidad sigue existiendo); atajos globales
  junto a Ctrl+S (con un campo de texto enfocado, las teclas
  son del campo); entradas Undo/Redo con estado en el menú
  Edit; **vigía** al final de `render`: reinicia el historial
  si cambia el nombre de la escena activa (carga aplicada por
  la app) y lo limpia al entrar en Play. **New Scene** (File)
  reinicia en el acto.

El enfoque de instantáneas + vigía evita instrumentar cada
widget: el Inspector y el gizmo mutan la escena directamente,
y cada gesto (crear, borrar, duplicar, arrastrar un valor,
mover con el gizmo, renombrar al perder el foco) genera
exactamente un paso.

## Verificación en vivo (editor real, Xvfb + xdotool)

Ciclo completo sobre la escena de ejemplo: selección de la
cámara (id 5) por raycast en la vista 3D → borrado desde el
menú Edit → el menú Undo se habilita (el vigía consolidó el
paso) → **Undo de menú: la cámara vuelve**, en su posición y
orden originales → **Redo de menú: se borra de nuevo** →
**Ctrl+Z de teclado: vuelve** → **Ctrl+Y de teclado: se
borra** → Ctrl+Z final: escena completa. Capturas en
`fase1/test-undo/` (fuera del repo).

Notas de la sesión:

- El borrado de `SceneState` usa `swap_remove`: al borrar la
  primera entidad, la última ocupa su lugar en la lista. Las
  instantáneas restauran el orden exacto, así que el deshacer
  también corrige ese reordenamiento.
- En esta VM, `xdotool key ctrl+z` (instantáneo) no registra
  la combinación: hay que mantener Ctrl pulsado entre frames
  (`keydown ctrl` → `key z` → `keyup ctrl`). Es un artefacto
  del entorno sin gestor de ventanas, no del editor: la tecla
  simple (E/W del gizmo) y el atajo con Ctrl mantenido
  funcionan.
- **Hallazgo para el bloque siguiente (Supr)**: clic en una
  fila del Hierarchy fija `hierarchy.selected_entity`, pero la
  sincronización de fin de frame en `render` lo sobrescribe
  con el `selected_id` local (que solo actualizan la vista 3D
  y el Inspector): la selección desde el Hierarchy **no
  persiste** y el Inspector queda vacío. La selección desde la
  vista 3D sí funciona. Es la causa raíz probable de que Supr
  «no hiciera nada»: sin selección efectiva no hay nada que
  borrar. El arreglo (que la selección sea de verdad única)
  va en el bloque de Supr, como pidió D3m3tr10.

## Fuera de alcance v1

- Operaciones de archivos del panel Project y el editor de
  grafos de shaders (documento aparte).
- Cambios de scripts en Play (el historial se limpia al
  entrar en Play).

Consumo: DeepSeek 6.170 tokens (módulo `undo.rs` completo, a
la primera). Claude 0 · Kimi 0 · Opus 0.
