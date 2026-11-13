# Registro — Tecla Supr y selección única del editor (2026-11-13)

La tecla Supr borra la selección desde cualquier parte del
editor, y la selección es por fin una sola en los tres sitios
(vista 3D, Hierarchy, Inspector), como pidió D3m3tr10.

## Diagnóstico (verificado en vivo en la sesión del undo)

Supr «no hacía nada» por dos causas encadenadas:

1. **La selección desde el Hierarchy no persistía.** El clic
   en una fila fija `hierarchy.selected_entity`, pero la
   sincronización de fin de frame en `EditorDockingLayout::
   render` lo sobrescribía siempre con el `selected_id` local,
   que solo actualizan la vista 3D (picking por raycast) y el
   Inspector. Tras clicar en el Hierarchy, el Inspector se
   quedaba en «Ningún GameObject seleccionado»: no había
   selección efectiva que borrar.
2. **El atajo vivía dentro del panel.** La comprobación de
   `Key::Delete` estaba en `HierarchyPanel::render`, atada a
   esa selección local.

## Arreglo

- **`docking.rs` — fusión de selección en dos sentidos**: al
  empezar el frame se anota la selección inicial; al cerrar,
  si el valor del panel difiere del inicial, el cambio fue
  del panel y manda el suyo (el compartido se reinicializa de
  él al frame siguiente); si el panel no lo tocó, adopta el
  valor compartido (vista 3D / Inspector). De paso, esto
  también rescata la selección que fija la instanciación de
  prefabs del panel Project, que sufría la misma sobrescritura.
- **`docking.rs` — Supr global**: en la sección de atajos del
  marco (junto a Ctrl+Z), con la misma guarda de campo de
  texto enfocado y solo en modo Edit; borra la entidad de la
  selección única y limpia la selección. El borrado entra en
  el historial de deshacer (Ctrl+Z lo resucita).
- **`hierarchy.rs`**: retirada la comprobación local de la
  tecla; el borrado por menús (acción `HierarchyAction::
  Delete`) no cambia.

## Verificación en vivo (editor real, Xvfb + xdotool)

1. Clic en la fila «Player 3D Cube» del Hierarchy: la fila
   **queda marcada** con el ratón lejos y el Inspector
   muestra sus componentes (Transform, Mesh Renderer,
   RigidBody, Script, AudioSource). Antes: Inspector vacío.
2. Con el ratón sobre la Consola (foco lejos del Hierarchy),
   **Supr borra el Player**: la fila desaparece.
3. **Ctrl+Z lo resucita**: jerarquía completa de nuevo.
4. Picking de la vista 3D sin regresión: clic en el cubo →
   Inspector poblado y la fila correspondiente **también se
   marca en el Hierarchy** (la unificación es en los dos
   sentidos).

Capturas en `fase1/test-supr/` (fuera del repo).

Consumo: DeepSeek 0 · Claude 0 · Kimi 0 · Opus 0
(integración directa de Rayo: fusión de selección y atajo
global, sin generación delegada).
