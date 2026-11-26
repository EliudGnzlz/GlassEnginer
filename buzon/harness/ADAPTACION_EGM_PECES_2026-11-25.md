# Auditoría y adaptación — Elephant-Goldfish Model (EGM) de Google

Pedido de D3m3tr10 (2026-11-25): buscar el skill «elefante y pez», auditarlo y
adaptarlo al harness — no traerlo completo. Regla suya vigente: ninguna skill o
patrón de terceros se importa tal cual; antes, auditoría de seguridad (qué
instrucciones haría obedecer al agente, qué scripts trae, qué dominios toca).

Fuente auditada: `google/elephant-goldfish-model`, clonado en
`~/workspace/egm-audit/` solo para lectura, rama **main @
`46933754e63c0424daa9c270d7e0734245d79560`** (2026-11-25). **No se ejecutó ni
instaló nada.** Ojo: el tag v0.1.1 (6 nov 2026) no es idéntico a main — trae 7
skills, sin `egm-lite`, que se añadió después. Los recuentos de este documento
son los de main @ 4693375, que es lo que se leyó. *(Procedencia corregida tras
la revisión de los peces, §7.)*

## 1. Qué es

Un método de trabajo con tres roles:

- **Elefante**: la sesión larga con todo el contexto, que piensa y redacta el
  plan/documento de diseño.
- **Peces de colores (goldfish)**: sesiones nuevas **sin memoria** que reciben
  solo el documento final y lo revisan para romperlo. Hay **8 revisores**
  (ficheros en `agents/`; los otros dos ficheros de esa carpeta,
  `bootstrap_orchestrator` y `synthesizer`, no revisan): comprensión (¿se
  entiende sin contexto?), crítico de diseño (supuestos sin prueba, riesgos),
  zoom-out (sobreingeniería, YAGNI, reuso), preparación de implementación
  (¿se puede construir de una pasada?), detalles del código real,
  estructural, catalizador inventivo (TRIZ) y estilo.
- **Resolver**: en el original lo hace el elefante con su skill `egm-resolve`,
  pero la aprobación final es del humano («You»): el método trae una puerta
  de usuario antes de aplicar correcciones. Nuestra adaptación mueve ese
  peso al orquestador y solo deja llegar al usuario lo escalado (§4), por la
  regla de autonomía de D3m3tr10 — es una diferencia deliberada, no una
  transcripción.

El paquete trae 8 skills (en main; 7 en el tag v0.1.1), 8 personas de revisor,
estándares de documento y adaptadores para varios harnesses (Claude Code,
Codex, Cursor, Copilot, Windsurf, OpenCode/Gemini y otros; varios marcados
como experimentales por el propio proyecto).

## 2. Auditoría de seguridad

**Composición.** El núcleo del método es texto: Markdown y JSON. El
repositorio completo trae además código ejecutable Python (empaquetado,
instalador, CI, tests, `pyproject.toml`) y ficheros YAML/TOML/SVG/PNG. No hay
binarios. *(Corregido tras los peces: la primera versión de esta auditoría
decía «todo es texto», y no era exacto.)*

**Scripts.** Solo utilidades del propio plugin: empaquetar releases
(`scripts/pack_plugin.py`, `prepare_release.py`), un instalador experimental
para OpenCode (`adapters/opencode/install.py`: copia archivos a una carpeta
nueva, se niega a sobrescribir, y configura a los revisores con lectura
denegada de `.env`) y automatización de CI de su repositorio
(`.github/scripts/review_pr.py`). Nada de eso forma parte del método ni se
necesita para adaptarlo.

**Red — dicho con precisión.** El núcleo del método no hace red: los peces
originales son subagentes locales y los informes se escriben a archivos
locales. Pero el repositorio sí contiene llamadas de red en su tooling:
`review_pr.py` (CI) llama a `generativelanguage.googleapis.com` y a
`api.github.com`, y la skill `egm-review`, ante una URL aprobada por el
usuario, descarga la página y la guarda como fichero en el directorio de
trabajo. Nada de eso se adopta. *(Corregido tras los peces: la primera
versión afirmaba «no hay llamadas de red» a secas.)*

**Higiene que ya trae (a su favor).** La skill de revisión trata los enlaces
del documento con cuidado: los que salen del directorio de trabajo o son URL
exigen aprobación del usuario; sin supervisión, se quedan fuera. Los peces son
de **solo lectura** por mandato (no escriben, no compilan, no arreglan).

**Instrucciones que impondría si se importara tal cual (el riesgo real).** El
archivo compartido `resources/execution_instructions.md` y las skills imponen
conductas pensadas para su harness, no para el nuestro:

- Puertas interactivas constantes: parar y esperar respuesta del usuario en
  cada etapa (pregunta única, sin multi-selección). **Choca con la regla de
  autonomía de D3m3tr10**: la cola no se para por decisiones operativas y se
  avanza entre fases sin esperarlo.
- «Artifact-first» obligatorio: prohibido mostrar informes en el chat; todo a
  archivos. **Choca con nuestros cierres**, que se entregan en el chat con su
  línea de consumo.
- Prohibiciones de tono (adjetivos validadores y disculpas vetados, «nunca
  estar de acuerdo por defecto» como postura global). Es policía de estilo de
  otro; nuestra voz ya está fijada en SOUL.md.
- Su `AGENTS.md` es normativa para contribuir a SU repositorio (CLA de Google,
  reglas de PR): ajeno por completo.
- Skills extra que no necesitamos: `egm-bootstrap` (escanear repos) y
  `egm-drift-report` (deriva plan/código) añaden superficie sin hueco real.

**Veredicto de la auditoría:** el método es sano y su núcleo es portable, pero
el paquete es un harness completo disfrazado de plugin. Se adapta el núcleo y
se descarta el resto.

## 3. Lo que ya tenemos (no duplicar)

- El elefante ya existe: la sesión larga (chat principal / chat del motor /
  Análisis) que redacta planes y expedientes con D3m3tr10.
- La revisión cruzada P4 ya manda un segundo proveedor con contexto fresco
  sobre el **resultado** de paquetes críticos (máx. 2 rondas).
- El contrato de paquete, el presupuesto duro y el reporte de consumo ya
  existen.

**El hueco real que EGM cubre:** nadie rompe el **plan** antes de construir,
con contexto cero y por escrito. Los planes grandes (prefabs, fase UI, banco
de la ECU) se revisaron a veces con Opus, pero sin aislamiento de contexto,
sin taxonomía de resolución y sin puerta formal.

**Glosario mínimo** (para que este documento se sostenga solo, sin contexto
externo — exigencia que los peces hicieron valer sobre el propio documento):
*revisión cruzada P4* = la regla vigente de mandar un segundo proveedor, con
contexto fresco, a revisar el resultado de un paquete crítico (máx. 2 rondas);
*contrato de paquete* = la ficha única que define cada encargo a un proveedor
(objetivo observable, contexto exacto, rutas permitidas y prohibidas,
presupuesto, formato de salida, validación); *presupuesto duro* = tope de
tokens por paquete: al llegar, se para y se entrega parcial con el motivo;
*regla anti-saturación* = decisión de D3m3tr10 de no crear skills nuevas para
cada regla: lo nuevo vive como sección de las skills existentes; *línea de
cierre* = la línea de consumo de tokens por proveedor que acompaña a todo
cierre de bloque.

## 4. La adaptación propuesta

Una sección nueva en la skill `orchestrated_code_delegation`, hermana de la
revisión cruzada y construida sobre su misma maquinaria (llamada a proveedor
con contexto fresco, contrato de paquete, presupuesto duro): lo que cambia es
el artefacto — el **plan antes de construir**, no el resultado después. Sin
skill nueva, por la regla anti-saturación. Texto listo para insertar tras la
sección de revisión cruzada:

---

### Revisión pez del plan (adaptada de EGM, 2026-11-25)

- **Umbral.** Pasan por peces los planes que cumplan cualquiera de estas tres
  cosas: (1) definen un bloque o una fase completa; (2) tocan cimientos
  compartidos (formato de escena, física, superficie MCP, pipeline de build);
  (3) comprometen al usuario en una compra o en una construcción de varias
  sesiones. Un paquete suelto y acotado dentro de un bloque ya aprobado no
  supera el umbral: le toca la variante ligera (abajo).
- **Los peces.** Llamadas a proveedores con contexto fresco que reciben SOLO
  el documento del plan (y los archivos base que el contrato liste), nunca la
  conversación ni el razonamiento que lo produjo. Si un pez no puede aislarse
  del contexto, no es un pez: se dice, la puerta no se da por pasada y, en un
  plan sobre el umbral, el trabajo se detiene y se escala — no se disfraza.
  Los peces son de solo lectura por construcción: devuelven texto; no
  escriben, no ejecutan, no arreglan.
- **Criba de salida.** Antes de enviar, el orquestador revisa el plan y los
  archivos base: nada de credenciales, claves, datos personales ni secretos
  sale a un proveedor. Los planes son documentos de diseño; si un plan
  necesitara un secreto para entenderse, se redacta esa parte antes de
  enviar. Los proveedores son los ya conectados al harness (sus dominios ya
  están auditados como conectores).
- **Mínimo y lentes.** Dos peces de **familias de proveedor distintas** como
  mínimo (familia = proveedor, no tamaño de modelo; p. ej. DeepSeek y Claude;
  Opus si el plan es crítico). El contrato asigna las lentes por escrito y la
  cobertura mínima es esta: el **pez de comprensión recibe el documento
  SOLO**, sin archivos base (como en el original: su objeto es probar que el
  plan se sostiene sin contexto), y primero parafrasea el plan con sus
  palabras — si no puede, o el parafraseo sale torcido, eso ya es un
  hallazgo bloqueante; el segundo pez lleva la lente **crítica** (supuestos
  sin prueba, riesgos sin mitigar, costes y presupuestos); la lente
  **zoom-out** (sobreingeniería, reuso antes de construir, ¿ataca la raíz o
  el síntoma?) la lleva el segundo pez o un tercero. En planes con detalle de
  implementación se añade la lente de **preparación** (¿se puede construir de
  una pasada con lo que el plan dice?; contrasta el plan con el código
  existente): solo ese pez recibe los archivos base que su lente necesita.
- El pez no reescribe el plan: solo señala huecos, con severidad
  (Bloqueante / Mayor / Menor) y el supuesto o fallo concreto. Si no hay
  tantos hallazgos, no se inventan. Cada pez es un paquete con su contrato y
  su `presupuesto_tokens`, y su consumo entra en la línea de cierre.
- **Resolución antes de construir.** El orquestador clasifica cada hallazgo en
  exactamente una categoría y la deja escrita en una **sección de resolución
  al final del propio documento del plan** (no intercalada: el plan se sigue
  leyendo limpio, y la re-auditoría recibe el plan corregido SIN esa sección,
  para que el historial no contamine al siguiente pez):
  **APLICAR** (válido: se corrige el plan), **REBATIR** (solo con evidencia
  comprobable que esté dentro de lo que el pez recibió — el propio plan o los
  archivos base del contrato — citada de forma que un tercero pueda
  comprobarla: archivo, símbolo o decisión registrada; «fue intencionado» no
  es evidencia; si la refutación necesita algo que el pez no recibió, el plan
  estaba incompleto: es APLICAR, añadiendo esa evidencia al plan),
  **ESCALAR** (solo decisiones de diseño o conocimiento que solo tiene el
  usuario — sube a D3m3tr10; un hallazgo válido que al orquestador no le
  gusta no es ESCALAR: es APLICAR o REBATIR con evidencia) o **ANOTAR**
  (compromiso aceptado, no se corrige: solo para Menor, y para Mayor con la
  justificación escrita en la resolución; **nunca para un Bloqueante**).
- Reglas duras de la resolución: un hueco de comprensión nunca se rebate (si
  un lector fresco no lo siguió, al plan le falta contexto); no se cambia la
  severidad de un hallazgo, se rebate o se aplica; haber escrito el plan no
  es evidencia contra el pez. Todo Bloqueante rebatido se nombra expresamente
  en el resumen de cierre del chat, para que D3m3tr10 pueda revisarlo.
- **Re-auditoría.** Máximo **una** tras corregir (endurecimiento deliberado
  frente al original, que permite una por etapa: aquí la puerta es una sola
  pasada y no puede costar como un bloque). Si la re-auditoría devuelve un
  Bloqueante, **no se construye**: se corrige y se escala si la corrección
  exige una decisión del usuario. Lo demás que quede se anota según la regla
  de ANOTAR.
- Los supuestos que el plan no puede verificar se marcan en el propio plan
  como `[SIN VERIFICAR]`, cada uno con qué lo verificaría y cuándo (un test
  del propio bloque o una fecha). Construir sobre un `[SIN VERIFICAR]` que
  afecte a dinero o a seguridad se escala antes, no se anota.
- **Variante ligera** para paquetes medianos: un solo pez con lente crítica,
  que primero parafrasea en breve (la paráfrasis es la evidencia de lectura) y
  después da su veredicto binario — «conforme» o solo hallazgos; nunca las
  dos cosas. Los hallazgos se resuelven por escrito en el contrato del
  paquete. Sin re-auditoría.
- **Periodo de prueba.** La puerta nace a prueba durante los **3 primeros
  planes** que superen el umbral: si en ese periodo los hallazgos útiles no
  superan claramente al ruido, la puerta vuelve a D3m3tr10 para ajustarla o
  retirarla, con los números de esos tres planes.

---

### Qué cambia respecto al original, y por qué

| Del EGM original | En nuestra adaptación | Por qué |
|---|---|---|
| 8 skills + 8 personas + estándares | 1 sección en la skill existente; 3–4 lentes como texto de prompt | Regla anti-saturación: cero skills nuevas |
| Puertas interactivas en cada etapa y aprobación del humano en la resolución | Resolución automática del orquestador; solo ESCALAR llega al usuario; los Bloqueantes rebatidos se nombran en el cierre | Regla de autonomía: la cola no se para por lo operativo; la mitigación sustituye a la puerta sin copiarla |
| Informes siempre a archivos, nada en chat | Hallazgos y resolución en la sección final del plan; el resumen va en el cierre del chat con consumo | Nuestros cierres se entregan en chat |
| Peces = subagentes del mismo harness | Peces = llamadas a otros proveedores (familias distintas) | Nuestro «contexto fresco» real es otro proveedor; además cubre puntos ciegos entre familias |
| Revisión en 5 etapas con checkpoints; 1 re-auditoría por etapa | Una pasada de peces + resolución + máx. 1 re-auditoría global | Presupuesto: la puerta no puede costar como un bloque (endurecimiento declarado) |
| Lentes estructural e inventiva (TRIZ) | No se adoptan | Zoom-out cubre reuso y sobreingeniería en nuestros planes; TRIZ es ajeno a nuestro flujo — compromiso anotado, no descuido |
| Brainstorm/draft del elefante | No se adopta: el elefante ya es nuestro flujo | No duplicar |
| Prohibiciones de tono y disculpas | No se adopta | Nuestra voz ya está fijada; no es mecanismo, es estilo ajeno |
| `egm-bootstrap`, `egm-drift-report` | No se adopta | Sin hueco real en el harness |

## 5. Coste esperado

Cada pez lee un plan (5–30 KB) y devuelve hallazgos cortos: del orden de
10–40k tokens por pez según el plan — cifra `[SIN VERIFICAR]`: es una
estimación sin medición, y los tres planes del periodo de prueba la
recalibran con el consumo real (que ya entra en la línea de cierre). Una
revisión completa de un plan de bloque (2–3 peces + posible re-auditoría)
debería quedar muy por debajo de un paquete de generación típico, y solo se
dispara sobre el umbral — no en cada paquete.

## 6. Aplicación (OK de D3m3tr10, 2026-11-25 19:34 — HECHO)

Con el OK: se inserta la sección en `orchestrated_code_delegation`, se anota
en `~/AGENTS.md` la referencia — texto exacto: «La revisión pez del plan
(adaptación EGM 2026-11-25) vive en la skill `orchestrated_code_delegation`;
el documento de auditoría y resoluciones es
`~/workspace/ADAPTACION_EGM_PECES_2026-11-25.md`» — y se registra la decisión
en memoria. El clon de auditoría se queda en `~/workspace/egm-audit/` como
evidencia, sin instalar (compromiso anotado: ocupa espacio a cambio de
procedencia reproducible, con su commit fijado en el encabezado).

## 7. Resolución de los peces (2026-11-25)

El propio documento pasó por la puerta que propone, antes de aplicarse: dos
peces sin contexto sobre la primera versión — el CLI de Antigravity en modo
plan (Gemini, cuenta de D3m3tr10) y el agente de investigación en modo tarea
(que además verificó el repositorio original en vivo). Veredictos: NO
CONFORME (2 bloqueantes) y NO CONFORME (5 bloqueantes). Resolución por
hallazgo (convergencias agrupadas):

| # | Hallazgo (fuente, severidad) | Categoría | Resolución |
|---|---|---|---|
| 1 | Tras la re-auditoría, «lo que quede se anota y se construye» convertía Bloqueantes vivos en ANOTAR (ambos peces, Bloqueante) | APLICAR | ANOTAR queda prohibido para Bloqueante y tasado para Mayor; Bloqueante tras re-auditoría = no se construye (§4) |
| 2 | REBATIR pedía evidencia «que el pez pudo haber leído» sin delimitarla frente al aislamiento (Antigravity, Bloqueante) | APLICAR | Evidencia válida = solo lo que el pez recibió; si está fuera, el plan estaba incompleto y es APLICAR; si el pez no puede aislarse, la puerta no se da por pasada y se escala (§4) |
| 3 | La adaptación envía planes y archivos a proveedores externos sin criba, mientras §2 celebraba la ausencia de red (modo tarea, Bloqueante) | APLICAR | Nueva regla de criba de salida en §4 y §2 reescrito con precisión |
| 4 | 2 peces × 1 lente no cubren 3–4 lentes; el ejemplo confundía tamaño con familia (ambos, Bloqueante) | APLICAR | Cobertura mínima asignada por escrito; familia = proveedor; comprensión siempre con el documento solo (§4) |
| 5 | Inventario de la auditoría falso para la versión citada y procedencia irreproducible (modo tarea, Bloqueante) | APLICAR | Recuentos corregidos en §1–§2 y fuente fijada: main @ `4693375`, con la diferencia del tag v0.1.1 declarada (encabezado) |
| 6 | Jerga sin definir (P4, contrato, anti-saturación…): el documento no se sostenía solo (ambos; Bloqueante en modo tarea) | APLICAR | Glosario mínimo en §3. Por la regla dura, un hueco de comprensión no se rebate: se corrige |
| 7 | Dar archivos base a todos los peces rompe la lente de comprensión del original (modo tarea, Mayor) | APLICAR | El pez de comprensión recibe el documento solo; los archivos base van solo a la lente que los necesita (§4) |
| 8 | Orquestador autor y juez único, sin la puerta de usuario del original (ambos, Mayor) | APLICAR (parcial) | Mitigación sin copiar la puerta: REBATIR exige cita comprobable por un tercero y todo Bloqueante rebatido se nombra en el cierre del chat (§4). La puerta interactiva sigue sin adoptarse por la regla de autonomía |
| 9 | ESCALAR redefinido permitía sacar hallazgos válidos como «decisión de diseño» (modo tarea, Mayor) | APLICAR | ESCALAR queda limitado a diseño o conocimiento exclusivo del usuario, con la exclusión escrita (§4) |
| 10 | Umbral no operativo (ambos, Mayor/Menor) | APLICAR | Umbral en tres criterios comprobables (§4) |
| 11 | `[SIN VERIFICAR]` sin dueño ni plazo; construir sobre él sin regla (ambos, Mayor) | APLICAR | Cada marca lleva qué la verifica y cuándo; si afecta a dinero o seguridad, se escala antes de construir (§4) |
| 12 | Duplicación frente a P4: ¿por qué no adelantar P4 al plan? (ambos, Mayor) | REBATIR | Evidencia: P4 revisa el **resultado** ya construido; el hueco (§3) es el plan **antes** de construir — otro artefacto y otro momento. La adaptación no duplica la maquinaria: la reutiliza (misma llamada fresca, contrato y presupuesto) y §4 ahora lo dice expresamente. La alternativa «P4 sobre el plan» es, en sustancia, esta misma sección |
| 13 | Coste 10–40k/pez sin medición ni comparador (ambos) | APLICAR | La cifra queda marcada `[SIN VERIFICAR]` en §5 y se recalibra con los 3 planes del periodo de prueba |
| 14 | Sin piloto ni criterio de retirada de la puerta (modo tarea, supuesto) | APLICAR | Periodo de prueba de 3 planes con vuelta a D3m3tr10 si el ruido gana (§4) |
| 15 | Variante ligera sin lente, sin evidencia de lectura y sin resolución definida (ambos, Menor/Mayor) | APLICAR | Lente crítica, paráfrasis breve previa, resolución escrita en el contrato (§4) |
| 16 | Lentes structural e inventive_catalyst omitidas sin decirlo; pérdida de la lente de código real (modo tarea, Mayor) | ANOTAR / APLICAR | Omisión declarada como decisión en la tabla de cambios; la lente de preparación absorbe el contraste con el código (§4) |
| 17 | La resolución escrita dentro del plan lo contamina y la re-auditoría la leería (modo tarea, Menor/supuesto) | APLICAR | La resolución va en sección final separada y la re-auditoría recibe el plan sin ella (§4) |
| 18 | Conservar el clon completo en el workspace (Antigravity, Menor) | ANOTAR | Se conserva como evidencia con el commit fijado; coste de espacio aceptado (§6) |
| 19 | El original asigna la resolución/aprobación al humano y permite 1 re-auditoría por etapa (modo tarea, verificación) | ANOTAR | Diferencias declaradas en §1 y en la tabla de cambios: son decisiones de la adaptación, no transcripciones |
