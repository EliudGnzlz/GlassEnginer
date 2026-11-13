# REGISTRO — Eventos de animación (2026-11-13)

Primer ítem del bloque de animación diferida, por orden de D3m3tr10
(2026-11-13): empezar por los eventos, el hueco barato señalado en
la auditoría del módulo, y evaluar el resto de la lista diferida
(evaluación en `LISTA_DE_DESPUES.md` §4b, a la espera de su
decisión sobre StateMachineBehaviours).

## Diseño

Paquete de diseño de Claude Sonnet 5 (14.767 tokens): eventos en el
propio contexto de reproducción (estado o hijo de blend), no en un
registro global por clip — el mismo clip en dos contextos puede
llevar eventos distintos. Correcciones del integrador: comparación
en tiempo absoluto (no normalizado) y descarte del campo `entity`
en el evento disparado (la salida de `advance` ya es por entidad).

## Implementación

- **Datos** (`crates/glass_core/src/animator.rs`): `AnimEvent
  { name, time (0..1), value: Option<f32>, text: Option<String> }`
  en `StateDef.events` y `BlendChild.events`, ambos
  `#[serde(default)]`: los controladores existentes cargan igual.
- **Disparo puro** en `AnimatorData::advance`: `AdvanceOutput`
  gana `fired_events: Vec<FiredAnimEvent>`. La función libre
  `recolectar_eventos` decide el cruce con el primer instante de
  disparo estrictamente posterior a `prev` (en bucle, módulo la
  duración; sin bucle, contra el clamp). Puntos de evaluación: el
  clip del estado actual (con su speed y su clamp/wrap), los hijos
  de su blend tree **solo con peso > 0** en el frame, y el origen
  del fundido con sus propios relojes (sigue disparando mientras
  mezcla, aunque el fundido se complete en ese frame). Regla
  documentada: **máximo 1 ocurrencia por evento y frame** aunque el
  dt salte varios ciclos.
- **Entrega a C#** espejando `DispatchContactEvent`: el host
  resuelve el export gestionado `DispatchAnimEvent` con tolerancia
  (ensamblado viejo = sin eventos, el motor arranca igual) y
  `ScriptHost::dispatch_anim_event(entidad, nombre, valor, texto)`.
  La app drena los eventos tras el pase de la FSM, fuera del
  bloqueo de la escena. En C#, `RuntimeBridge.DispatchAnimEvent`
  busca el behaviour de la entidad y le invoca el virtual nuevo
  `GlassBehaviour.OnAnimationEvent(string, float?, string?)`,
  con excepciones capturadas y registradas.
- **Telemetría MCP**: módulo `glass_core::animator_stats` (anillo
  global de 128, patrón de `lighting_stats`);
  `glass_animator_state` incluye `recent_events` (últimos 10 de la
  entidad: estado, clip, nombre, datos, instante y nº de orden).

## Verificación

- **9 tests unitarios nuevos** sobre `advance`: cruce simple en
  bucle, disparo al envolver el ciclo, no-loop una sola vez tras el
  clamp, hijo de blend con reloj sin envolver, hijo sin peso no
  dispara, dt gigante = 1 ocurrencia, speed 2 llega antes, el
  origen del fundido sigue disparando, sin cruce no hay eventos.
  **Suite completa: 419/419** (410 + 9).
- **Prueba física** (`PruebaEventosAnim.scene.json`, 2 X Bot):
  la entidad 4 con el controlador `eventos_test` (blend tree,
  Speed = 2,5) disparó **PasoIzq** (instante 0,2604 = 0,25 × 1,0417 s
  del clip Walk) y **PasoDer** (0,7813, con `value` y `text`)
  alternando cada ciclo; la entidad 5 con `eventos_simple` (estado
  simple) disparó **PasoSimple** (0,5208 = 0,5 × duración) por
  ciclo. El guión `EventosAnimTest.cs` recibió **44 eventos** por
  `OnAnimationEvent` con sus datos intactos (log de la app), y la
  telemetría MCP los mostró con su orden global.

## Consumo

- Diseño: **Claude Sonnet 5: 14.767 tokens**.
- Código: **DeepSeek Flash: 26.804 útiles** (datos+helper 4.198 ·
  evaluación+tests 16.219 · host+app 4.288 · C# 2.099) y 10.363
  quemados en una entrega vacía (todo razonamiento, sin contenido).
- Claude: 3 intentos de paquetes de código muertos por el timeout
  de 120 s del cliente (la pasarela estaba en una oleada lenta),
  sin medición; el trabajo pasó a DeepSeek tras el primer fallo de
  cada paquete.
- Integración, arreglos (derives desplazados, literales de tests,
  `BlendTree`→`BlendTree1D`), escena, scripts y drivers: trabajo
  propio.
