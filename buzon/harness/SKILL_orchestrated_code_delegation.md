# Delegate code generation across providers

## Use one package contract for every dispatch

- No package leaves without its contract fields: id, observable objective, provider and model with the reason for that weight, the exact context inside the prompt file, the paths it may touch, the paths and interfaces it must not touch, its constraints, its token budget and max iterations (below), the required output format (full code, patch or report, plus assumptions and open risks), the validation command the orchestrator will run on integration, and whether it needs the cross-review gate.

- State the dependency line before any parallel fan-out: what is shared foundation (one owner, first) and what is truly independent.

## Give every package a hard budget

- Every package is born with `presupuesto_tokens` and `max_iteraciones` written in its contract, and every block carries a block-level ceiling as the sum of its packages.

- Check the accumulated usage between calls, not only at closure: token counts are the control, never step counts alone, because one call can cost many times another.

- At the ceiling, stop and deliver what exists as a partial result with the reason, instead of asking for more balance or quietly shifting the load to another provider.

- Keep the standing low-balance warning tied to this accounting so it fires from the numbers, not from memory.

## Cross-review critical packages before integrating

- Send a package through a second provider before integration only when it is critical or touches shared foundations: heavy generation, root fixes, or machinery that already-closed systems depend on.

- Give the reviewer fresh context with only the spec and the returned result or diff, never the first provider's reasoning, and require a plain verdict: conforme, or no conforme with the defects listed.

- Cap the review loop at two rounds; after that, the orchestrator decides and says why at closure.

- Leave medium and small packages to the orchestrator's own validation so the gate does not tax everything.

## Revisión pez del plan (adaptada de EGM, 2026-11-25)

- **Umbral.** Pasan por peces los planes que cumplan cualquiera de estas tres cosas: (1) definen un bloque o una fase completa; (2) tocan cimientos compartidos (formato de escena, física, superficie MCP, pipeline de build); (3) comprometen al usuario en una compra o en una construcción de varias sesiones. Un paquete suelto y acotado dentro de un bloque ya aprobado no supera el umbral: le toca la variante ligera (abajo).
- **Los peces.** Llamadas a proveedores con contexto fresco que reciben SOLO el documento del plan (y los archivos base que el contrato liste), nunca la conversación ni el razonamiento que lo produjo. Si un pez no puede aislarse del contexto, no es un pez: se dice, la puerta no se da por pasada y, en un plan sobre el umbral, el trabajo se detiene y se escala — no se disfraza. Los peces son de solo lectura por construcción: devuelven texto; no escriben, no ejecutan, no arreglan.
- **Criba de salida.** Antes de enviar, el orquestador revisa el plan y los archivos base: nada de credenciales, claves, datos personales ni secretos sale a un proveedor. Los planes son documentos de diseño; si un plan necesitara un secreto para entenderse, se redacta esa parte antes de enviar. Los proveedores son los ya conectados al harness (sus dominios ya están auditados como conectores).
- **Mínimo y lentes.** Dos peces de **familias de proveedor distintas** como mínimo (familia = proveedor, no tamaño de modelo; p. ej. DeepSeek y Claude; Opus si el plan es crítico). El contrato asigna las lentes por escrito y la cobertura mínima es esta: el **pez de comprensión recibe el documento SOLO**, sin archivos base (como en el original: su objeto es probar que el plan se sostiene sin contexto), y primero parafrasea el plan con sus palabras — si no puede, o el parafraseo sale torcido, eso ya es un hallazgo bloqueante; el segundo pez lleva la lente **crítica** (supuestos sin prueba, riesgos sin mitigar, costes y presupuestos); la lente **zoom-out** (sobreingeniería, reuso antes de construir, ¿ataca la raíz o el síntoma?) la lleva el segundo pez o un tercero. En planes con detalle de implementación se añade la lente de **preparación** (¿se puede construir de una pasada con lo que el plan dice?; contrasta el plan con el código existente): solo ese pez recibe los archivos base que su lente necesita.
- El pez no reescribe el plan: solo señala huecos, con severidad (Bloqueante / Mayor / Menor) y el supuesto o fallo concreto. Si no hay tantos hallazgos, no se inventan. Cada pez es un paquete con su contrato y su `presupuesto_tokens`, y su consumo entra en la línea de cierre.
- **Resolución antes de construir.** El orquestador clasifica cada hallazgo en exactamente una categoría y la deja escrita en una **sección de resolución al final del propio documento del plan** (no intercalada: el plan se sigue leyendo limpio, y la re-auditoría recibe el plan corregido SIN esa sección, para que el historial no contamine al siguiente pez): **APLICAR** (válido: se corrige el plan), **REBATIR** (solo con evidencia comprobable que esté dentro de lo que el pez recibió — el propio plan o los archivos base del contrato — citada de forma que un tercero pueda comprobarla: archivo, símbolo o decisión registrada; «fue intencionado» no es evidencia; si la refutación necesita algo que el pez no recibió, el plan estaba incompleto: es APLICAR, añadiendo esa evidencia al plan), **ESCALAR** (solo decisiones de diseño o conocimiento que solo tiene el usuario — sube a D3m3tr10; un hallazgo válido que al orquestador no le gusta no es ESCALAR: es APLICAR o REBATIR con evidencia) o **ANOTAR** (compromiso aceptado, no se corrige: solo para Menor, y para Mayor con la justificación escrita en la resolución; **nunca para un Bloqueante**).
- Reglas duras de la resolución: un hueco de comprensión nunca se rebate (si un lector fresco no lo siguió, al plan le falta contexto); no se cambia la severidad de un hallazgo, se rebate o se aplica; haber escrito el plan no es evidencia contra el pez. Todo Bloqueante rebatido se nombra expresamente en el resumen de cierre del chat, para que D3m3tr10 pueda revisarlo.
- **Re-auditoría.** Máximo **una** tras corregir (endurecimiento deliberado frente al original, que permite una por etapa: aquí la puerta es una sola pasada y no puede costar como un bloque). Si la re-auditoría devuelve un Bloqueante, **no se construye**: se corrige y se escala si la corrección exige una decisión del usuario. Lo demás que quede se anota según la regla de ANOTAR.
- Los supuestos que el plan no puede verificar se marcan en el propio plan como `[SIN VERIFICAR]`, cada uno con qué lo verificaría y cuándo (un test del propio bloque o una fecha). Construir sobre un `[SIN VERIFICAR]` que afecte a dinero o a seguridad se escala antes, no se anota.
- **Variante ligera** para paquetes medianos: un solo pez con lente crítica, que primero parafrasea en breve (la paráfrasis es la evidencia de lectura) y después da su veredicto binario — «conforme» o solo hallazgos; nunca las dos cosas. Los hallazgos se resuelven por escrito en el contrato del paquete. Sin re-auditoría.
- **Periodo de prueba.** La puerta nace a prueba durante los **3 primeros planes** que superen el umbral: si en ese periodo los hallazgos útiles no superan claramente al ruido, la puerta vuelve a D3m3tr10 para ajustarla o retirarla, con los números de esos tres planes.

## Split the work into bounded packages

- Plan the work yourself and prepare the exact context for each package before you call a provider.

- Keep each package to one bounded task with the exact code excerpts it needs instead of sending a whole session at once.

- Review and test each returned package yourself before you integrate it or pass the next package.

## Assign each package by weight

- Send every DeepSeek package to DeepSeek with `deepseek-flash` and never use a Pro model for DeepSeek.

- Send small packages and large context packages to Kimi with `kimi-k3.1-pro` and keep its reasoning effort low unless the package needs more.

- Send heavy or critical packages to Claude Opus and medium packages to Claude Sonnet.

- Keep surgical fixes with no provider call only when the correction is small and you can verify it yourself, and say so at closure.

## Check balance and health before you assign

- Check the balance of the assigned provider before you size a package to it.

- Size a DeepSeek package to its verified balance and report the package with its evidence when it does not fit instead of moving its load to Claude in silence.

- Investigate a provider failure in its helper, parameters and gateway before you move its package to another provider.

- Diagnose in this order: run a short test call to the same provider, check whether the failure is already reported, inspect the helper (streaming, timeouts, reasoning settings, max-tokens headroom) and the gateway, fix what is clearly broken, and retest small before retrying the real package.

- Splitting an oversized package into smaller bounded ones for the same provider counts as fixing it, not as substituting it.

- Move a package to another provider only when the failure is external and cannot be fixed here, and tell the user with the evidence.

## Report use at closure

- Report the token use per provider for the closed work in the closure message.

- Name the provider and model that produced each package beside its test and build evidence.

- Say plainly which packages stayed open when a provider was unavailable instead of calling the work closed.

## Available connectors

- Use `muse.connector_search` to find other connectors and `muse.skill_search` to find related skills.

- If nothing here fits, look on the web for a maintained API or MCP server and set it up with [Custom Connectors and MCPs](/opt/muse/skills/custom-connector-setup/SKILL.md), or build a custom connector.

## Alternatives for better results

- Split a large package into smaller bounded packages when one call hides where it failed.

- Ask the user to judge the result when automated checks cannot show whether it matches what was asked.
