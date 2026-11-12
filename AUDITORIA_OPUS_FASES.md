# AUDITORÍA OPUS — Fases cerradas 0–8P

**Fecha:** 2026-11-12. **Auditor:** `claude-opus-5-5` (10 paquetes acotados contra el código real y los `REGISTRO_FASE*.md`; el paquete monolítico inicial superó el timeout de lectura del conector y se rehízo por fragmentos). **Alcance:** prioridad alta — 2S, 1+1.5, 2, 6; media — 0, 3, 4, 5; costuras entre fases. **Solo análisis: ningún arreglo está aplicado.** Cada hallazgo lleva la severidad del auditor y la verificación del integrador (Rayo) contra el código: **Confirmado**, **Matizado** o **Refutado**.

**Consumo de Opus:** 119.155 tokens en 10 paquetes (todos `claude-opus-5-5`). Un primer intento monolítico murió por timeout del conector sin respuesta registrada.

## Resumen

- **Críticos: 3** — todos en el puente Blender/vigía de la Fase 6. Ningún crítico en física, shaders ni render.
- **Mayores: ~24** — los más serios: eventos de física que se pierden o se duplican con cuerpos compuestos, el acumulador de substeps que no descarta el exceso, los mips BC mal dimensionados en texturas NPOT, y la asimetría de colliders en el spawn en vivo (afecta también a `Instantiate` de C#).
- **Refutados tras verificar: 3** (el `Default` de colliders es correcto, el clamp de ToonBands sí está en la emisión, y las cotas de culling sí incluyen las transformaciones de nodos).

---

## CRÍTICOS — Puente Blender y vigía (Fase 6)

### C-1. Una exportación fallida puede aprobar el .glb viejo y firmar el sidecar como fresco — **Confirmado**
El script embebido imprime el inventario (líneas 90–92) **antes** de `bpy.ops.export_scene.gltf` (línea 94). Blender en modo `-b` sale con código 0 aunque el script lance (no se usa `--python-exit-code`), así que si la exportación falla, el bridge encuentra el inventario, valida el `.glb` **anterior** que sigue en disco y escribe el sidecar con el hash de la fuente nueva. A eso se suma que el SHA-256 de la fuente se calcula **después** de exportar: si el usuario guarda durante la exportación, el sidecar tampoco corresponde al .glb. *Impacto beta: sí — el motor puede dar por bueno un activo viejo o mezclado sin ningún aviso.*

### C-2. Exportaciones concurrentes del mismo activo sin exclusión — **Confirmado el mecanismo**
El vigía lanza un hilo por evento y no hay lock por ruta. Dos guardados seguidos con contenido distinto (o una lectura a mitad de escritura) lanzan dos Blender que escriben el mismo `.glb` y el mismo `.meta.json`: el orden de fin es arbitrario y puede quedar el GLB de un contenido con el sidecar de otro.

### C-3. El vigía recarga el .glb mientras Blender lo está escribiendo (y dos veces) — **Confirmado**
La rama `"glb"` del vigía emite `MeshReloaded` ante cada Create/Modify del `.glb` que el propio exportador está escribiendo por partes, y al éxito se emite otra vez desde la rama blend. El motor intenta cargar GLBs truncados y recarga dos veces cuando todo va bien.

## MAYORES

### Física (Fases 1 y 1.5)

- **M-1. El Exit de un cuerpo eliminado se pierde; el consumidor se queda con un Enter sin cerrar — Confirmado.** Al despawnear, Rapier emite `Stopped` con flag `REMOVED`, pero para entonces `entity_of_collider` ya devuelve `None` y el evento se descarta (el flag se ignora). Un script con estado "dentro del trigger" queda pegado. `contact_pairs` sí se limpia.
- **M-2. Enter/Exit se emiten por collider, no por par de entidades — Confirmado.** Con colliders compuestos (Fase 1.5), un par (a,b) recibe varios Enter y un Exit falso cuando un solo collider deja de tocar aunque otro siga en contacto.
- **M-3. El tope de substeps no descarta el acumulador — Confirmado en `world.rs::step`.** Tras un tirón (carga, alt-tab), el mundo ejecuta 4 substeps por frame hasta saldar la deuda; si el frame medio supera 4×fixed_dt, no salda nunca: cámara lenta permanente con CPU al máximo. Un `dt` NaN envenena el acumulador y congela la física.
- **M-4. Triggers frente a cuerpos cinemáticos no generan eventos — Confirmado (default de Rapier), impacto condicionado.** `ActiveCollisionTypes::default()` no incluye pares cinemático↔estático ni cinemático↔cinemático para sensores. Nuestro jugador es dinámico, así que Plat3D no lo sufre; una puerta o plataforma cinemática atravesando un trigger sí lo sufriría.
- **M-5. `attach_convex_collider` sin guarda anti-pánico — Confirmado el mecanismo.** A diferencia del trimesh (que sí valida), el hull de parry puede entrar en pánico con puntos NaN/inf o patológicos. La entrada es la geometría retenida por el motor, no datos crudos del usuario: el riesgo es un modelo importado con vértices no finitos.
- **M-6. Raycasts y consultas de forma golpean triggers — Confirmado.** Ninguna consulta usa `exclude_sensors`. Matiz del integrador: Unity también impacta triggers por defecto; aquí afecta de verdad a la **oclusión de audio** (un trigger grande puede "ocluir" una fuente) y a los barridos de esfera/cápsula.

### Shaders (Fase 2S)

- **M-7. El nodo `Property` no tiene guarda de etapa: en vértices emite `u_props` sin declarar — Confirmado.** `emit_property` no comprueba `stage` como sí hacen `TextureSample` y `PointLight*`. Una propiedad animando el desplazamiento de vértices produce un error crudo de glslang en vez de un `GraphError` claro. El chequeo `glass_shader_compile_check` lo caza, pero con mensaje opaco.
- **M-8. Grafo con `.spv` pero sin `.graph.json` registrado: pierde Shadow/Outline en silencio — Confirmado el mecanismo.** Pipelines y registro de pases se alimentan de fuentes distintas; si el pipeline existe, el fallback magenta no salta y nadie loguea que los pases cayeron a `[Forward]`: sombra con silueta PBR y sin contorno toon.
- **M-9. `"passes": []` explícito anula el pase Forward — Confirmado.** El default de serde solo aplica si el campo falta; un JSON editado a mano puede dejar la entidad invisible y solo con sombra, sin validación ni aviso.
- **M-10. El tope de 32 propiedades no se hace cumplir en el compilador — Confirmado (parcial).** `properties_ubo_size` crece sin límite; si el pool por instancia del renderer está dimensionado para 32, un grafo mayor desborda el bind. El tamaño real del pool no quedó verificado en esta pasada.

### Render (Fase 2)

- **M-11. La cadena de mips BC usa ceil; Vulkan usa floor — Confirmado.** Las dimensiones por nivel se calculan con `(w+1)/2` y la copia a GPU usa esos extents (mesh.rs ~1766). En texturas NPOT (p. ej. 18 px → nivel 2: CPU 5 px, Vulkan 4) la copia supera el mip: error de validación o mips corruptos. Las texturas POT (lo probado en la fase) no lo sufren. Las NPOT son comunes en glTF.
- **M-12. Respaldo de cotas de 1 m³ en el culling — Matizado.** Si una malla GPU existe sin geometría retenida, su AABB es 1 m³ en el origen local y un modelo grande puede desaparecer al salir su pivote de cámara. Verificado que carga y recarga insertan geometría y malla juntas y la expulsión las retira juntas: la frecuencia real es baja, pero el respaldo no es conservador.

### Assets (Fase 6)

- **M-13. Jerarquía cíclica en un GLB: recursión infinita y desbordamiento de pila — Confirmado el mecanismo (Opus: crítico).** `node_globals::walk` no tiene visitados ni límite. Exige un GLB corrupto o editado a mano (Blender no lo produce); el resultado es aborto del proceso, no un error limpio.
- **M-14. La fusión sin piel itera todos los nodos en plano — Confirmado.** Los nodos huérfanos (fuera de toda escena) se fusionan con identidad, perdiendo hasta su transformación local; con varias escenas, todas se fusionan superpuestas en vez de usar la escena por defecto.
- **M-15. Escala negativa (espejo) no invierte el winding — Confirmado.** Un nodo espejado hornea triángulos con el orden invertido: el culling oculta las caras frontales. Caso real en assets simétricos de Blender.
- **M-16. `lod_suffix` no detecta `_LOD<n>` con sufijo de duplicado de Blender (`.001`) ni en minúsculas — Confirmado.** `"Esfera_LOD2.001"` devuelve `None` y el nivel se dibuja siempre como malla normal: el LOD se pierde en silencio justo en el caso habitual de duplicar un objeto.
- **M-17. El hash se marca como exportado ANTES de exportar; si falla, nunca se reintenta — Confirmado.** `contenido_cambio` registra el hash al decidir, y ninguna rama de fallo lo revierte: guardar el mismo contenido tras arreglar el problema (instalar Blender, corregir el PATH) loguea "sin cambios" y el .glb sigue obsoleto hasta editar o reiniciar.
- **M-18. Los errores del bridge son invisibles en el log del motor — Confirmado.** El vigía loguea solo `stderr` del proceso, pero el bridge reporta todo por `stdout` (`GLASS_BRIDGE_RESULT`): ante "Blender no encontrado" o "GLB inválido", el log muestra un fallo con texto vacío.
- **M-19. Si la validación falla tras escribir, el .glb roto queda en disco — Confirmado.** Blender escribe directo a la ruta final; un GLB inválido o a medias queda junto al sidecar antiguo, sin restaurar nada.

### Audio (Fase 3)

- **M-20. Un duck solapado sobrescribe al anterior y el volumen salta a 1,0 — Confirmado.** El nuevo duck arranca su rampa desde 1,0 aunque el anterior esté en meseta: "pop" audible, y un duck suave anula a uno fuerte activo.
- **M-21. Ducks más cortos que 0,65 s tienen discontinuidades — Confirmado.** Las rampas fijas (0,25 + 0,4) no se recortan en proporción: con duración 0,5 hay un escalón a mitad; con menos de 0,25, un salto a 1,0 al final. Matiz: hoy los llamadores internos ponen el `start` con el reloj del propio sistema de audio (la "mezcla de relojes" que señala Opus no tiene hoy ningún llamador que la dispare).

### Animación (Fase 4)

- **M-22. Thresholds duplicados en un blend tree: saltos de pose e hijos que nunca se mezclan — Confirmado.** Con duplicados, el hijo anterior cambia de golpe al siguiente sin mezcla en el punto duplicado. Solo si el contenido trae duplicados.
- **M-23. El `exit_time` de un estado blend se mide con el clip del hijo de MENOR threshold — Confirmado (es la decisión documentada).** Si el personaje está corriendo (hijo alto), la transición por tiempo sale según la duración del idle: antes o después de lo esperado.

### UI (Fase 5)

- **M-24. Los nietos de un hijo de layout group no se recolocan — Confirmado.** El layout reescribe solo los rects de los hijos directos en el mapa; los descendientes ya se resolvieron contra el rect previo del hijo. Un botón movido por un layout deja su texto/icono en la posición antigua. (En la escena de cierre de la Fase 5 los botones partían casi en su sitio y el defecto no se vio.)
- **M-25. La escala sale del primer Canvas del vector, incluidos los inactivos — Confirmado.** Con dos canvas de distinta resolución de referencia, el segundo se escala mal; un canvas desactivado puede fijar la escala de todos.
- **M-26. Ciclo entre ancestros no-UI: bucle infinito en la resolución — Confirmado (parcial).** La guarda anti-ciclos solo protege a entidades con rect/canvas; el paseo por ancestros sin UI no tiene contador (la función de profundidad sí). Exige una jerarquía corrupta.

### Costuras entre fases

- **C3 (MAYOR, abierta). Sombras sin LOD:** el pase de sombras dibuja todos los niveles (coste × niveles, ×6 caras en puntuales) y la silueta es la unión: sombras más gruesas y posible acné al recibir la malla visible la sombra de otro nivel.
- **C4 (MAYOR, abierta, documentada). Spawn en vivo sin mesh colliders:** verificado en el código que la ruta en vivo (la que usan MCP **y el `Instantiate` de C#**) anota el `mesh_collider` pero cae a primitivos; solo la carga de escena lo resuelve. Un prefab con mesh collider instanciado en runtime queda con collider aproximado, y la misma entidad guardada y recargada se comporta distinto.
- **C7 (MAYOR, abierta). Preview de UI del editor ≠ runtime:** el editor conserva su fórmula histórica; con Canvas Scaler o layout groups, el preview miente (y con M-24/M-25, el runtime también tiene sus propios defectos que el preview no mostraría).
- **C9 (MAYOR para contribuidores, abierta, fuera del plan). Sin undo/redo** de entidades; las operaciones del MCP tampoco se pueden deshacer. Opus sugiere como mitigación barata un snapshot de escena antes de cada guardado o lote MCP.
- **C1 (parcial, Post-1.0 aceptado). Materiales por primitiva sin slots con nombre:** el dibujo por rangos funciona; la asociación es por ÍNDICE: reordenar materiales en Blender cruza los materiales de las partes al reexportar, sin aviso, y cada nivel LOD puede divergir.
- **C2 (MENOR, abierta). Recarga en caliente de shaders inexistente:** el evento se llama `ShaderReloaded` y el log lo "confirma", pero no se recrea ningún pipeline. El nombre engaña al depurar.
- **C5/C6 (MENOR, parciales/abiertas).** C#: `Physics` solo expone Raycast (sphere/capsule/overlap masked quedan solo en MCP) y `AudioSource` no tiene propiedad `Bus` (toda fuente de script cae en SFX y escapa al control del bus Music; un bus mal escrito en el JSON cae en silencio al default).
- **C8 (cerrada, con nota). Slots de sombras puntuales:** verificada la coherencia slot↔luz (un solo bucle ordenado por distancia alimenta UBO y lista). Nota de Opus: sin histéresis, las luces en el borde del cupo de 4 pueden parpadear al entrar/salir por distancia.

## MENORES (agrupados)

Física: umbral absoluto de degeneración en la guarda trimesh (rechaza mallas de escala ~mm); capa ≥32 truncada en silencio con `& 31`; duplicación de construcción de formas en `attach_collider_with_config_at`; cuaternión nulo → transform NaN; pares duplicados en `contact_pairs` si una entidad mezcla sólido y sensor contra el mismo par; `pending_contact_events` sin tope si nadie drena; handle de collider sin generación (ABA teórico: hoy nada borra colliders por handle, los cuerpos se borran enteros). Shaders: samplers/UBO de luces declarados por nodos huérfanos no alcanzables; `properties_ubo_size` puede ser 0 (un buffer de tamaño 0 lo rechaza Vulkan, si alguna ruta lo crea); variante de `PassKind` mal escrita invalida el grafo entero; registro de pases sin borrado (inocuo sin hot reload); `graph_error_logged` no se limpia si el grafo se arregla y se rompe en la misma sesión; doc-comments pegados a funciones equivocadas en `shader_graph.rs`. Render: mips BC promediados en espacio sRGB sin ponderar por alfa (mips lejanos más oscuros que en la ruta RGBA8); un solo píxel con alfa 254 fuerza BC3. Assets: LOD > 2 inalcanzable salvo que no exista ninguno ≤ 2; mallas estáticas dentro de un GLB con piel pierden su transformación de nodo (camino histórico); nodo con dos padres → global indeterminada; validación GLB superficial (no compara la longitud de cabecera con el archivo); `GLASS_BLENDER` inválido cae al PATH en silencio; bridge sin timeout si Blender se cuelga; escritura del sidecar no atómica. Audio: duración de duck 0/negativa cancela el duck activo con un salto; NaN en volumen/amount se propaga; `effective_volume` del snapshot no incluye Master (el debug puede mostrar un volumen que no es el oído). Animación: durante un fundido no se evalúan transiciones ni `Play`/`CrossFade` (se retrasan al final); blend→blend reinicia relojes sin sincronía de fase; `state_time` en loop crece sin wrap (precisión tras ~18 h); al cambiar de controller quedan restos de fundido. UI: un padre ausente resuelve contra la pantalla en silencio; los hijos inactivos ocupan hueco en el layout (Unity los ignora). Instrumentación (Fase 0): las estadísticas GPU se calculan sobre un subconjunto de frames y `last_gpu_render_us` no lleva antigüedad; sin timestamps, los ceros se serializan igual que valores reales (distinguible solo mirando `gpu_samples`).

## REFUTADOS tras la verificación del integrador

- **El `ColliderConfig::default()` NO es derivado:** su `impl Default` manual pone `mask: u32::MAX` (types.rs:231). Ningún collider sin config queda sordo.
- **ToonBands SÍ clampea al emitir** (`clamp(1, 16)` en el GLSL generado): un JSON con `bands: 0` no divide por cero.
- **Las cotas de culling SÍ incluyen las transformaciones de nodos:** salen de la geometría CPU fusionada (que ya lleva las globales aplicadas). Queda solo el matiz de la pose de piel (bind pose) como menor.
- **Slots de sombras puntuales coherentes** (ver C8).

## Verificado correcto por el auditor (sin hallazgos)

Física: todas las rutas de attach pasan por `apply_config` con los grupos correctos; la guarda trimesh valida antes de indexar; el cruce (friction, restitution) es uniforme dentro del crate; canales de eventos sin pánico; anillo de 256 correcto. Shaders: el orden posicional del UBO salta las texturas igual al compilar que al empaquetar; slot > 3 e índice de luz > 7 son errores de compilación; nodos de fragmento bloqueados en vértice (salvo `Property`, M-7). Render: elección BC1/BC3 por el alfa real; fallback del cubo coherente; `mip_level_count` correcto. Assets: mapeo nodo→rangos con piel por suma prefija correcto; inversa-transpuesta sobre la global compuesta correcta. Audio: Master inmune al duck; el duck es función pura sin estado pegado. Animación: limpieza de relojes al terminar el fundido. UI: orden de layout groups por profundidad; canvas como raíz. Instrumentación: avg/p95/max correctos incluso con pocas muestras.

## Lista para decisión (sin aplicar)

1. **Puente/vigía (C-1, C-2, C-3, M-17, M-18, M-19):** bloque de robustez del pipeline .blend — es el área más débil del motor hoy: `--python-exit-code`, hash de la fuente ANTES de exportar, exportar a temporal y renombrar, lock por ruta, no vigilar el .glb de salida como entrada, registrar el hash solo tras el éxito, y loguear el resultado del bridge.
2. **Física (M-1, M-2, M-3):** Exit sintético al despawnear, eventos agregados por par de entidades y descarte del exceso de acumulador. M-3 es de una línea y protege de tirones reales.
3. **Mips NPOT (M-11):** usar la regla floor de Vulkan al dimensionar niveles.
4. **LOD (M-16, C3):** aceptar `.001` tras el sufijo y aplicar el filtro LOD también en los pases de sombra.
5. **UI (M-24):** recalcular descendientes tras aplicar layout groups (o resolver en dos pasadas).
6. **Spawn en vivo (C4):** decidir si la ruta en vivo resuelve mesh colliders con la geometría retenida (la caché vive en la app; el puente necesitaría acceso) o si se documenta como límite de la beta.
7. **Resto de mayores:** por fase, cuando se toque cada sistema.
