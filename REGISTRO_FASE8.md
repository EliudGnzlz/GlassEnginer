# REGISTRO — Fase 8: Command Queue del MCP

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE8.md`. Especificación: §Fase 8 + anexo §8 del consolidado.

**Verificación de cierre: 347 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 42 → **45** (`glass_destroy_entity`, `glass_queue_submit`, `glass_queue_status`).

## Entregado

- **`glass_mcp::queue::CommandQueue`**: ids secuenciales, estados pending/running/done/error, resultado o error por comando, pasada de ejecución, marca de ejecución directa e historial de 200. Espera por Condvar y latido de bombeo (vivo si bombeó hace < 2 s). Tests unitarios del ciclo de vida y del timeout.
- **Bombeo en el bucle**: la app llama `mcp_server.pump_queue()` al inicio de `render_frame`, antes de física y scripts. Los comandos se ejecutan en orden de envío, en un punto fijo del frame, nunca en el hilo del cliente ni a mitad de frame.
- **Compuerta en el enrutador** (`handle_request` → `call_tool`): una tool mutante con bombeo vivo se encola y el cliente espera su resultado (15 s), conservando la respuesta síncrona de siempre; sin bombeo vivo se ejecuta directa y queda registrada (`direct: true`). Lista explícita de 17 tools mutantes; las de consulta quedan directas, y `glass_simulate_player` también (es una sonda que ejecuta la física ella misma).
- **`glass_queue_submit`**: lote asíncrono validado por completo antes de encolar (solo mutantes; el lote entero se rechaza si algo no es encolable). **`glass_queue_status`**: registro por comando o resumen con los últimos 10.
- **`glass_destroy_entity`**: el MCP no exponía destrucción aunque `SceneState::delete_entity` existía; la secuencia de cierre del plan la necesitaba.

## Test físico (VM; driver `fase1/driver_f8.py`, carpeta `test-fisico8/`)

Secuencia del plan (spawn → mover → consultar → destruir → consultar) sobre Fase2Demo en vivo:

1. `glass_create_entity` síncrono: pasó por la cola como **comando 1** y se ejecutó en la **pasada 134**; el cliente recibió su respuesta normal («id=216») — la compuerta conserva la semántica.
2. `glass_queue_submit` con una tool de consulta dentro → **rechazado (−32008) sin encolar nada**.
3. Lote de dos `glass_modify_component` (posición [3, 1.5, 2] y rotación 45°): comandos 2 y 3, ejecutados **en orden en la misma pasada 135**. La captura muestra el cubo creado, en primer plano en su posición movida y girado 45°.
4. Consulta `glass_queue_status`: 3 done, 0 errores, con los resultados guardados por comando.
5. Lote con `glass_destroy_entity`: **comando 4, pasada 151**, «Entidad 216 destruida»; el estado final lo confirma.

Nota del primer intento: el driver extrajo mal el id (leyó el id JSON-RPC en vez del de la entidad) y movió/destruyó la entidad 2 de la sesión en vivo; la escena en disco no se tocó y la sesión se descartó. La segunda corrida, con el id correcto, es la verificada arriba.

## Deuda anotada

- Las llamadas internas `handle_json` desde el propio hilo de render (autocomprobaciones de arranque) dependen de que el bombeo aún no esté vivo en ese momento para ir directas; si algún día se invocan con el bucle en marcha, esperarían hasta el timeout de 15 s.
- Un lote encolado no puede referenciar ids creados por comandos anteriores del mismo lote (el cliente conoce el id solo cuando el comando se ejecuta): patrón correcto = crear síncrono y encolar el resto.

## Consumo (de los `out*.json` de la fase)

- **Claude: 24.156 tokens** (cola completa + compuerta + tools; un par de la struct no casó por un carácter y se aplicó a mano).
- **Kimi: 2.085 tokens** (docs/COMMAND_QUEUE.md).
- DeepSeek: 0. El bombeo en la app y el test físico fueron de Rayo.
