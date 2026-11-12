# REGISTRO — Fase 4: Animación, cross-fade y blend tree 1D

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE4.md`. Especificación: §Fase 4 + anexo §8 del consolidado.

**Verificación de cierre: 336 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 35 → **37** (`glass_animator_state`, `glass_animator_set_param`).

## Punto de partida (verificado en código)

La FSM v1 ya traía el **cross-fade con slerp** completo: `AnimatorData::advance` producía clip origen/destino con peso y el renderer mezclaba poses con slerp en rotaciones. Lo que faltaba de la fase era el **blend tree 1D** y las **tools** del §8. La fase generaliza la salida de la FSM a N samples, que es lo que ambos (fundidos y árboles) necesitaban.

## Entregado

- **Blend tree 1D** (`glass_core::animator`): `StateDef.blend: Option<BlendTree1D>` con `BlendChild { clip, threshold, speed }`. `blend_weights(tree, valor)` pura y pública: hijos ordenados por umbral, reparto lineal entre vecinos, clamp a los extremos fuera de rango. Cada hijo tiene **su propio reloj** (`AnimatorData.blend_times`; los del origen de un fundido viven en `fade_blend_times` y se mueven al iniciar el fundido), avanzado con `dt × speed del estado × speed del hijo`.
- **`AdvanceOutput` generalizado** a `samples: Vec<ClipSample { clip, time, weight }>`: un estado normal aporta un sample a peso 1; un estado blend, sus hijos con peso; durante un fundido, los samples del origen × (1−w) más los del destino × w (suma 1). Los tests de la FSM v1 se migraron a esta forma y se añadió el test del árbol (pesos 0,5/0,5 a mitad de tramo, clamp fuera de rango).
- **Renderer**: `evaluate_skin_matrices_multi(skin, &[(clip, tiempo, peso)])` — traslación/escala como media ponderada normalizada y rotación por **slerp iterativo por el camino corto** (con N=2 reproduce el blended histórico; test de equivalencia incluido). `evaluate_skin_matrices_blended` queda como envoltorio.
- **App**: `anim_pose` pasa a `Vec<(clip, tiempo, peso)>` por entidad y el bucle de dibujo llama a `multi`. El crossfade libre sin controlador no se toca.
- **MCP**: `glass_animator_state` (estado, tiempo, fundido con origen y peso, parámetros y — si el estado es blend — hijos con peso y reloj, cargando el controlador de `assets/animators/`) y `glass_animator_set_param`.
- **Contenido**: controlador `assets/animators/locomocion.animator.json` (Idle ↔ Locomocion por el bool `walk` con fundido 0,3; Locomocion = blend por `Speed`: Idle@0, Walk@2,5, `Armature|mixamo.com|Layer0`@5) y escena `Fase4Blend` con el xbot. Docs: `docs/ANIMATION.md` (Kimi).

## Test físico (VM; driver `fase1/driver_f4.py`, capturas en `test-fisico4/`)

Todo por MCP sobre la escena viva:
1. Estado inicial **Idle**, sin blend ✓.
2. `walk=1` → estado **Locomocion** con el árbol activo (Speed 0 → hijo Idle a peso 1) ✓.
3. Rampa de Speed medida con la tool: **1,25 → Idle 0,5 / Walk 0,5**; **2,5 → Walk 1,0**; **3,75 → Walk 0,5 / Layer0 0,5**; **5 → Layer0 1,0**. Los relojes de los hijos avanzan ✓.
4. `walk=0` en plena mezcla → la tool caza el fundido de vuelta (**fading_from: Locomocion, peso 0,33**) y termina en Idle ✓.
5. Capturas a Speed 0 / 1,25 / 2,5: el ángulo de los brazos interpola de forma progresiva entre las poses de Idle y Walk (horizontal → diagonal → abajo), sin saltos ✓.

Nota de activo (verificada numéricamente en el .glb, no es un defecto del motor): el clip `Idle` de `xbot_fsm.glb` mantiene los brazos casi en la pose de bind (desviación máxima de 0,038 en el cuaternión del brazo en todo el clip, frente a 0,249 en Walk): es un idle de respiración muy sutil y por eso a Speed 0 el personaje se ve con los brazos abiertos. El clip `Layer0` comparte la pista del brazo con Walk en este ensamblado.

## Incidencias

- La primera entrega de Claude (núcleo) dejó el bloque viejo de construcción de la muestra duplicado tras el nuevo (su VIEJO del paso 3 no cubría el bloque entero): se retiró el huérfano y las aserciones de tests que no aplicaron se migraron a mano.
- El frame que dispara una transición ahora descarta el sample del destino con peso 0 (antes el clip destino se nombraba con peso 0): sin efecto visual; el test del trigger se ajustó a la nueva semántica.

## Consumo (de los `out*.json` de la fase)

- **Claude: 20.991 tokens** (núcleo del blend tree en la FSM).
- **DeepSeek: 4.264 tokens** (`evaluate_skin_matrices_multi` + tests, completo a la primera).
- **Kimi: 2.712 tokens** (docs/ANIMATION.md).
