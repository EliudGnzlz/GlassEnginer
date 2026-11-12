# Command Queue del MCP

## El problema

Hasta la Fase 8, cada tool MCP se ejecutaba inmediatamente en el hilo del cliente, incluso a mitad del trabajo del motor. Las tools que **alteran el estado** ahora pasan por una Command Queue: el bucle del motor las ejecuta en orden de envío, al inicio del frame y antes de física y scripts, nunca a mitad de frame.

## Funcionamiento y bombeo

El módulo `glass_mcp::queue` proporciona `CommandQueue`. Asigna ids secuenciales desde `1` y conserva los últimos 200 comandos.

Cada comando registra su tool, argumentos, estado (`pending`, `running`, `done` o `error`), resultado o error, pasada de bombeo en que se ejecutó y si fue directo. Las consultas responden directamente y no alteran el orden de los comandos.

## Caminos del cliente

### 1. Llamada normal a una tool mutante

Si el bucle está bombeando —latido inferior a 2 s—, la llamada se encola y el cliente espera su resultado hasta 15 s. Recibe la respuesta habitual: la semántica síncrona no cambia, pero la ejecución ocurre ordenadamente dentro del bucle.

Si la espera vence, se devuelve el error `-32007` con el `command_id`. El comando **sigue encolado**.

### 2. Envío de un lote sin espera

`glass_queue_submit` acepta `{commands: [{tool, args}, …]}`, encola el lote y devuelve los `command_ids` sin esperar su ejecución.

La validación se completa antes de encolar nada. Solo admite tools mutantes: una consulta, una tool de la propia cola o una desconocida rechazan el lote entero con `-32008`.

## Estados y consulta

`glass_queue_status` permite inspeccionar la cola:

- Con `command_id`: devuelve el registro completo de ese comando.
- Sin `command_id`: devuelve un resumen (`pending`, `done`, `errors`, `total`), la pasada actual, si el bombeo vive y los últimos 10 comandos.

## Modo sin bucle

En modo `--mcp` stdio puro o durante el arranque, las mutantes se ejecutan directamente como antes. Quedan en el historial con `direct=true` y `pass=0`.

## Tools mutantes

- `glass_create_entity`, `glass_batch_spawn_entities`, `glass_destroy_entity`.
- `glass_modify_component`, `glass_set_ui_text`, `glass_set_entity_material`.
- `glass_material_property_set`, `glass_shader_node_add`, `glass_shader_pass_configure`.
- `glass_shader_preset_apply`, `glass_animator_set_param`.
- `glass_audio_buses`, `glass_play_audio`, `glass_reload_shaders`.
- `glass_bake_lightmap`, `glass_setup_platformer_scene`, `glass_asset_reload`.

`glass_simulate_player` también se ejecuta directamente: es una sonda que ejecuta la física por sí misma.

## Destrucción de entidades

La Fase 8 añade `glass_destroy_entity` con `{entity_id}`. El motor ya podía destruir entidades mediante `SceneState::delete_entity`; esta tool expone esa operación al MCP y la ejecuta como comando de la cola.