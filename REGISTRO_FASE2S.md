# REGISTRO — Fase 2S: rediseño de shaders (S0–S6)

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE2S.md`. Orden interno ejecutado: S0 → S1 → S2 → S4 → S3 → S5 → S6, con la migración de la librería dentro de S0. Dirección de arte: ambas líneas (PBR y toon/estilizado), techo sin fotorrealismo.

**Verificación de cierre: 327 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"` y `DOTNET_ROOT` en el entorno). Tools MCP: 24 → **34** (10 nuevas de la fase).

## Entregado por bloque

- **S0 — Propiedades de grafo.** `ShaderGraph.properties: Vec<PropertyDef>` (`Float|Color|Vec2|Vec3|Vec4|Texture2D`); el compilador emite el UBO `GraphProperties` (set 1, binding 1) solo si hay propiedades no-textura; cada propiedad ocupa un `vec4` posicional `p<i>` (tope 32), así renombrar no rompe el SPIR-V. Valores por entidad en `MeshData.shader_properties`; UBO por instancia de material con pool de 64 sets en el renderer. Registro global de esquemas en glass_core y `pack_property_values` puro y testeado. Tools: `glass_shader_schema_get`, `glass_material_property_set`.
- **Migración de la librería.** Los 6 presets Rust declaran sus constantes principales como propiedades con el default = valor anterior. De los grafos de disco, brisa/espuma/fragmentos/tormenta_arena eran idénticos a sus presets y se adoptó el JSON migrado con SPV recompilado; agua/fuego/laser/olas_prueba son contenido evolucionado y quedan sin migrar.
- **S1 — Muestreo de texturas.** Nodo `TextureSample { slot }`: slot 0 = textura base del modelo; slots 1..=3 = propiedades Texture2D (samplers `tex_prop_<k>` en set 0, bindings 1..=3). Los pipelines de grafo llevan el set 0 con 4 bindings; la AssetCache crea los sets de grafo y la app resuelve la textura efectiva (override o default) por entidad. Tools: `glass_shader_graph_validate`, `glass_shader_node_add`.
- **S2 — Nodos de iluminación.** `MainLightDirection`, `MainLightColor`, `PointLightColor { index }`, `PointLightPosition { index }` (índice > 7 = error de compilación). El UBO de puntuales (set 3) se declara solo si el grafo lo usa. Nuevo módulo `glass_core::lighting_stats` y tool `glass_render_lighting_debug`.
- **S4 — Compilación comprobable y fallback.** Tool `glass_shader_compile_check` (parsea + compila el grafo y comprueba su `.spv`). Si el SPV de un grafo falta o no está registrado, la entidad se dibuja con el material PBR plano **magenta** y el error se registra una sola vez por grafo.
- **S3 — Pases de grafo.** `ShaderGraph.passes: Vec<PassKind>` (Forward|Shadow|Outline; JSON viejo sin `passes` = [Forward]). El pase Shadow usa la etapa de vértices del grafo en la profundidad del sol; recorte alfa opcional por la propiedad especial `alpha_clip` (fragmento `depth_clip.frag`, umbral por push constants). Límite v1: los proyectores de grafo no participan en las sombras de puntuales. Tools: `glass_shader_pass_configure`, `glass_render_pass_stats` (contadores `graph_shadow_draw_calls` y `outline_draw_calls` en render_stats).
- **S5 — Toon y contorno.** Nodo `ToonBands { bands }` (cuantiza un float en N bandas, 1..=16). Pase Outline: segundo dibujo con la malla desplazada por la normal × `outline_width`, cull frontal y color `outline_color` (shaders `outline.vert/frag` dedicados); v1 sin desplazamiento de vértices del grafo. Presets por grafo (`<grafo>.<preset>.preset.json`) y tool `glass_shader_preset_apply`.
- **S6 — Ordenación y medición de enlaces.** Los lotes estáticos se ordenan (estable) por (transparente al final, shader, textura) antes de los pases de vista y de los portales. El renderer cuenta los enlaces reales de pipeline y de descriptor set del bucle de mallas; `render_stats` los expone y la tool `glass_render_draw_stats` los reporta. Verificado: en la escena toon, 8 draws forward se graban con **6 enlaces de pipeline** (3 por vista: PBR, grafo, contorno) en vez de 8.

## Tests físicos (VM, lavapipe; capturas en `fase1/test-fisico2s/`)

- S0: dos cubos con el mismo grafo `tinte_demo`, uno con el rosa por defecto y otro verde por override; `glass_material_property_set` lo pasa a azul **en vivo** (`captura_s0_c.png`, `captura_s0_despues.png`).
- S1: el cubo sin textura muestra el damero procedente de la propiedad Texture2D; con override a textura inexistente sale blanco (`captura_s1.png`).
- S2: un cubo sale naranja con el color de la luz puntual 0 y otro blanco cálido con el del sol; `glass_render_lighting_debug` devuelve los valores exactos (`captura_s2.png`).
- S4: el cubo con `graph:no_existe` se ve magenta y el log de error aparece exactamente 1 vez (`captura_s4.png`).
- S3: la tabla con pase Shadow proyecta la silueta **ondulada** de su desplazamiento de vértices y la gemela sin pase, un rectángulo plano (`captura_s3.png`); el panel con `alpha_clip` 0.5 proyecta un damero agujereado y con 0.0, sombra sólida (`captura_s3_clip.png`). `glass_render_pass_stats`: graph_shadow_draw_calls = 3.
- S5: esfera toon con bandas planas y contorno oscuro junto a su gemela PBR de degradado suave; el preset «suave» aplicado por MCP afina el contorno y lo pasa a morado **en vivo** (`captura_s5_antes.png`, `captura_s5_despues.png`). outline_draw_calls = 1.
- Cierre combinado (escena Fase2SDemo): cambio de color por MCP en vivo, `glass_shader_compile_check` del grafo roto = no listo, draw/pass stats y lighting debug coherentes en la misma sesión (`captura_cierre_despues.png`).

## Incidencias de la fase

- Las **rotaciones de escena son GRADOS**, no radianes: una escena de prueba puso el sol bajo el horizonte y durante un rato pareció que «las sombras del sol no funcionan». Funcionan (verificado en Plat3D_L1 y en las demos S3). Registrado en el esquema de escena.
- Al añadir `passes` a `ShaderGraph`, un barrido de literales por conteo de llaves corrompió `shader_graph.rs`; se reconstruyó la región de presets desde la salida de la migración. Lección: procesar solo spans internos y verificar por tramos con el compilador.
- Bug cazado en el test físico de S3: la creación del pipeline de profundidad de grafo estaba condicionada a que el grafo tuviera etapa de vértices; los grafos sin ella no proyectaban sombra. Corregido usando el vértice PBR por defecto, como en el pase hacia adelante.
- Entregas de las APIs corregidas en integración: DeepSeek inventó anclas y APIs en varios paquetes (el primero de S6 se descartó entero: 13.829 tokens; reenviado con los VIEJO literales funcionó), DS-2S4b2 quemó 18.997 tokens en razonamiento sin contenido (se partió el paquete), un handler de DS-2S5c llegó ausente y se transcribió desde la especificación, y Claude se negó una vez a modificar código sin verlo (6.255 tokens; reenviado con el código real pegado, funcionó).

## Consumo (suma de los `out*.json` de la fase)

- **Claude: 165.644 tokens** (arquitectura de compilador S1/S2/S3/S5, pipeline de contorno).
- **DeepSeek: 117.133 tokens** (tools MCP, migración, cableados, S6; incluye 32.826 quemados en los paquetes fallidos citados).
- **Kimi: 0** en esta fase.
