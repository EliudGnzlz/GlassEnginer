# AUDITORÍA OPUS — Fases cerradas 0–8P

**Fecha:** 2026-11-12. **Auditor:** `claude-opus-5-5` (10 paquetes acotados contra el código real y los `REGISTRO_FASE*.md`; el paquete monolítico inicial superó el timeout de lectura del conector y se rehízo por fragmentos). **Alcance:** prioridad alta — 2S, 1+1.5, 2, 6; media — 0, 3, 4, 5; costuras entre fases. Cada hallazgo lleva la severidad del auditor y la verificación del integrador (Rayo) contra el código: **Confirmado**, **Matizado** o **Refutado**.

**Precisión de D3m3tr10 (2026-11-12, posterior al informe):** los hallazgos de prioridad alta (2S, 1+1.5, 2, 6) y las costuras críticas que los afectan se **arreglan en el acto** dentro de la misma pasada; los de prioridad media (0, 3, 4, 5) van a la lista diferida. La pasada de arreglos se ejecutó el mismo día: ver la sección siguiente.

**Consumo:** auditoría Opus 119.155 tokens (10 paquetes, todos `claude-opus-5-5`); pasada de arreglos: Opus 34.636 + DeepSeek 18.759 = 53.395 tokens. Un primer intento monolítico de auditoría murió por timeout del conector sin respuesta registrada.

## Pasada de arreglos — hallazgo → arreglo aplicado (prioridad alta)

Verificación de la pasada: suite completa **363 tests, 0 fallos** (354 previos + 9 nuevos de la auditoría); test físico de física (driver de la Fase 1, con estrés de 1000 cuerpos) en verde; `glass_asset_validate`/`glass_asset_reload` del activo de la Fase 6 en verde con el puente nuevo; pases de sombra de grafo en vivo (`graph_shadow_draw_calls = 3`) tras el cambio de C3.

**Críticos (puente Blender/vigía):**
- **C-1 → ARREGLADO.** El bridge lanza Blender con `--python-exit-code 1` (una excepción del script ya no sale 0), imprime el inventario DESPUÉS de exportar, calcula el hash de la fuente ANTES de exportar (ese va al sidecar; si la fuente cambia durante la exportación, el resultado lo marca `source_changed_during_export`) y valida también la longitud de cabecera del GLB.
- **C-2 → ARREGLADO.** El vigía lleva una guarda de exportaciones en vuelo por ruta (un segundo evento sobre la misma ruta se ignora) y el hilo, tras el éxito, re-lee el hash: si la fuente cambió durante la exportación, reexporta (máx. 3 rondas).
- **C-3 → ARREGLADO.** La rama «glb» del vigía no emite recarga si la ruta está en vuelo (es la salida que el exportador está escribiendo); la recarga la emite la rama blend/fbx al éxito. Además el bridge exporta a un temporal hermano y solo hace `os.replace` tras validar (vale también para M-19: un fallo ya no deja un .glb roto en la ruta final), y el sidecar se escribe igual, vía temporal.

**Física (M-1..M-6) → ARREGLADOS, con tests nuevos (`crates/glass_physics/tests/audit_fixes_test.rs`):**
- **M-1/M-2:** los eventos se agregan por par de entidades (`pair_colliders`/`pair_owner`/`pair_trigger` en el mundo): un Enter por par aunque haya varios colliders, Exit solo cuando el último collider deja de tocar, y el `Stopped` de un cuerpo eliminado resuelve su par por el mapa de dueños → el Exit llega. Tests: compuesto de 2 colliders = 1 Enter; despawn en contacto = 1 Exit.
- **M-3:** `step()` descarta el exceso de acumulador al hitting el tope de 4 substeps, y un `dt` no finito o negativo se trata como 0 (test: `step(10.0)` = 4 substeps y el siguiente paso normal no arrastra deuda; `step(NaN)` no envenena).
- **M-4:** `apply_config` fija `ActiveCollisionTypes::all()`: los sensores emiten eventos también frente a cinemáticos y estáticos (test: sensor estático × cuerpo cinemático teletransportado dentro → Enter).
- **M-5:** `attach_convex_collider` valida la entrada (vacío, <4 puntos, no finitos → None) y envuelve el hull de parry en `catch_unwind`: un pánico de parry se convierte en None con aviso, no en aborto.
- **M-6:** las cinco construcciones de `QueryFilter` llevan `.exclude_sensors()` (test: un rayo atraviesa un sensor y golpea el sólido de detrás). Nota: es un cambio de semántica deliberado frente al default de Unity; los triggers ya no bloquean raycasts ni barridos — en particular, ya no producen falsas oclusiones de audio.

**Shaders 2S (M-7..M-10) → ARREGLADOS, con tests nuevos en `shader_graph.rs`:**
- **M-7:** el nodo `Property` en la etapa de vértices devuelve `GraphError::FragmentOnlyNode` (test). **M-9:** `register_graph_passes` normaliza una lista vacía a `[Forward]` con aviso (test). **M-10:** `validated_maps` rechaza más de 32 propiedades de valor con el error nuevo `TooManyProperties` (test: 33 → error). **M-8:** al arrancar, la app cruza los `graph_*.frag.spv` presentes con los pases registrados y avisa una vez por grafo sin `.graph.json` («se asume [Forward], sin pase Shadow ni Outline»).

**Render (M-11, M-12) → ARREGLADOS, con tests:**
- **M-11:** `downsample_rgba` usa la regla de Vulkan `max(1, floor(w/2))`; la cadena BC de una textura NPOT ya coincide con los mips de la imagen (test: 18×10 → 9×5 → 4×2 → 2×1 → 1×1; el test histórico de 3×3 se actualizó a la nueva semántica).
- **M-12:** sin cotas retenidas, la entidad ya NO se recorta (respaldo ±1e6 = siempre visible) en vez del cubo de 1 m³ que podía hacer desaparecer modelos grandes.

**Assets (M-13..M-18) → ARREGLADOS:**
- **M-13:** `node_globals` lleva guarda de ancestros (un ciclo se trata como hoja, con aviso) y recorre solo la escena por defecto si existe. **M-14:** un nodo sin global usa su transformación LOCAL en la fusión, no la identidad. **M-15:** `merge_primitive` detecta determinante negativo (espejo) e invierte el orden de índices por triángulo. **M-16:** `lod_suffix` quita el sufijo de duplicado de Blender (`.001`) y acepta minúsculas (tests nuevos). **M-17:** el hash solo se registra tras el éxito (ver C-2); un fallo ya no bloquea el reintento. **M-18:** el fallo del bridge se loguea con su línea `GLASS_BRIDGE_RESULT` o la cola de stdout, no con un stderr vacío.

**Costuras que afectan a módulos de prioridad alta:**
- **C3 → ARREGLADO.** Los pases de sombra (sol y puntuales) dibujan solo los rangos del nivel LOD activo cuando el modelo tiene LOD (`ShadowCasters.statics` lleva los rangos; helper `rangos_sombra_lod` con la misma regla que el dibujo principal). Sin LOD, la malla entera como siempre. Los proyectores de GRAFO siguen dibujando la malla entera (combinación grafo+LOD: queda anotada en la diferida).
- **C4 → ARREGLADO.** El spawn en vivo resuelve el collider de malla igual que la carga de escena: `EngineSceneBridge` comparte la caché de activos con la app, la spec lleva modelo y escala de mundo, y `create_body` adjunta TriMesh (solo estáticos) o Convex con la geometría retenida, sustituyendo a los primitivos si cuela — el `Instantiate` de C# y el spawn por MCP quedan simétricos con la escena autorada.

## Lista diferida — RESUELTA en la pasada de medios (2026-11-12, decisión de D3m3tr10)

Toda la lista se trabajó, cada punto con sus tests. Solo **C1 queda fuera** (Post-1.0, como ya estaba aceptado). Suite final de la pasada y verificación física al final de esta sección.

- **Fase 3 (audio) → ARREGLADO.** M-20: `Duck` guarda el factor vigente al arrancar (`from`) y la rampa parte de ahí: los ducks solapados ya no saltan a 1,0. M-21: las rampas se recortan en proporción a la duración (`min(0,25, d·0,4)` / `min(0,4, d·0,4)`), sin escalones en ducks cortos. Menores: una duración ≤0 o no finita y un `amount` NaN se ignoran (ya no cancelan el duck activo); `set_volume` ignora NaN; el snapshot distingue el volumen efectivo interno (`effective()`, sin Master, el que usa el backend) del **percibido** (`snapshot_for`, con Master aplicado), que es el que se expone a depuración. 4 tests nuevos.
- **Fase 4 (animación) → ARREGLADO.** M-22: `blend_weights` colapsa los thresholds duplicados y manda el primero del grupo: sin saltos de pose ni hijos mudos. M-23: el `exit_time` se mide con el clip del hijo **dominante por peso** (`state_reference_clip`), no con el de menor threshold; la fracción usa el tiempo envuelto en estados con loop. Menores: durante el fundido también se evalúan `pending_play` y las transiciones (si una dispara, el fundido se completa en el acto); `state_time` se envuelve por la duración en loop sin blend; al cambiar de controlador se podan el fundido con origen inexistente y los relojes de clips ajenos. 4 tests nuevos.
- **Fase 5 (UI) → ARREGLADO.** M-24: `resolve_scene_ui_rects` itera a punto fijo (máx. 4 pasadas) con un mapa de rects impuestos por los layout groups que la resolución de cada entidad consulta antes que el cálculo por anclas: los nietos se resuelven contra el rect final del hijo. M-25: la escala se toma del Canvas **activo** más cercano a cada entidad (cada subárbol con la suya; un Canvas inactivo no impone nada; sin Canvas, escala 1). M-26: el paseo de ancestros que busca el ancestro UI lleva guarda de 64 pasos, también entre no-UI. Menores: los hijos con `active == false` quedan excluidos de los layout groups (como en Unity); un padre ausente del mapa ya no se resuelve contra la pantalla en silencio — avisa una vez por entidad y proceso. 5 tests nuevos.
- **Fase 0 (instrumentación) → ARREGLADO.** `FrameTimingSnapshot` lleva `last_gpu_age_frames`: la antigüedad en frames de la última muestra GPU dentro de la ventana, para distinguir un cero real de un dato viejo. Test de estadísticas extendido.
- **Menores de módulos de prioridad alta → ARREGLADOS.** Guarda trimesh con umbral **relativo** a la escala de la propia malla (las mallas milimétricas ya no se rechazan; test nuevo); `apply_config` avisa con `warn` si la capa es ≥32 (el truncado `& 31` se mantiene, pero ya no es silencioso); `contact_pairs()` fusiona el par sólido+sensor en una sola entrada y manda el contacto sólido (test nuevo); los samplers de grafo y el UBO de luces solo se declaran para nodos **alcanzables** desde el Output (BFS por las aristas) — un `TextureSample` huérfano ya no declara sampler (test nuevo); una variante de `PassKind` mal escrita se salta con `warn` y ya no invalida el grafo entero (deserializador tolerante; test nuevo); los mips BC se promedian en espacio **lineal** (EOTF sRGB exacta al bajar y al volver) y el alfa usa umbral `< 250` (254 ya cuenta como opaco y no fuerza BC3); los niveles LOD > 2 son alcanzables con bandas geométricas tras `LOD_FAR` (×2 por nivel, tope 16); en un GLB con piel, las mallas que ningún nodo con piel usa se fusionan **con la transformación global de su nodo** y pesos cero (el shader las dibuja sin deformar), en vez de perder el transform (test nuevo con GLB mixto); los proyectores de grafo en el pase de sombras dibujan por los rangos del nivel LOD activo (resto de C3, cerrado).
- **Costuras → ARREGLADAS (salvo C1).** **C2:** el vigía emite `ShaderReloaded` también para `.spv`, y la app recarga de verdad los materiales de grafo (`reload_graph_materials`) cuando cambia un `graph_*.spv`; para el resto de shaders el log dice honestamente que van en el binario y no hay recarga. El nombre del evento ahora es cierto. **C5:** la física expone a C# `Physics.SphereCast` y `Physics.OverlapSphere` (FFI `glass_physics_cast_sphere` / `glass_physics_overlap_sphere`, callbacks añadidos AL FINAL de la struct del puente, delegados y envoltorios en `RuntimeBridge`). **C6:** `AudioSource.Bus` existe en C# (defecto "Sfx"), `Play` enruta por los nuevos FFI `glass_audio_play_2d_on_bus` / `_3d_on_bus` y `PlayClipAtPoint`/`Play2D` aceptan bus opcional; un nombre de bus desconocido avisa y cae a Sfx (esto cubre también el menor del bus mal escrito en JSON por el lado de scripts). El puente de audio pasa a struct de callbacks como el de física: la resolución de delegados de netcorehost tiene un tope de aridad que los 7 punteros sueltos superaban. **C7:** el preview de UI del editor usa el mismo `resolve_scene_ui_rects` del runtime: preview y juego resuelven igual. **C9:** `SceneState::save_to_json` (el punto único de guardado) copia antes la versión anterior a `<escena>.snapshots/` y poda a las 10 más recientes por escena; un fallo de la copia nunca bloquea el guardado (test nuevo).
- **C1 → SIGUE DIFERIDA (Post-1.0 aceptado):** slots de material por nombre en primitivas; la asociación sigue siendo por índice.

**Hallazgo extra de la pasada (fuera de la lista):** el helper de tests `make_bridge` (introducido con C4 en la pasada alta) envolvía dos veces el `AssetCache` — cuyo `new()` ya devuelve `Arc<RwLock<Self>>` — y **el binario de tests de `glass_app` no compilaba**: esos tests de spawn no corrieron en la pasada alta aunque la suite se reportó en verde (el conteo de 363 salió de los paquetes que sí compilaron). Corregido en esta pasada; la suite completa de abajo incluye por fin ese objetivo.

**Verificación de la pasada de medios:** suite completa `cargo test --workspace` con `RUSTFLAGS="-D warnings"` y `DOTNET_ROOT`: **381 tests, 0 fallos** (antes 363, con el objetivo de `glass_app` sin compilar). El proyecto gestionado compila con 0 errores. Test físico: driver de la Fase 1 (demo de eventos con scripts C# + estrés) — ver el registro de la pasada en el historial de git. **Consumo de APIs en esta pasada:** Claude Opus 5-5: 12.168 tokens (paquete FIXUI, el rework del resolvedor de UI); el resto fueron arreglos quirúrgicos acotados de integración. DeepSeek 0, Kimi 0.

---

## Resumen original de la auditoría (antes de la pasada de arreglos)

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

## Lista para decisión original (histórica — los puntos 1–6 se resolvieron en la pasada de arreglos; el resto vive en la lista diferida de arriba)

1. **Puente/vigía (C-1, C-2, C-3, M-17, M-18, M-19):** bloque de robustez del pipeline .blend — es el área más débil del motor hoy: `--python-exit-code`, hash de la fuente ANTES de exportar, exportar a temporal y renombrar, lock por ruta, no vigilar el .glb de salida como entrada, registrar el hash solo tras el éxito, y loguear el resultado del bridge.
2. **Física (M-1, M-2, M-3):** Exit sintético al despawnear, eventos agregados por par de entidades y descarte del exceso de acumulador. M-3 es de una línea y protege de tirones reales.
3. **Mips NPOT (M-11):** usar la regla floor de Vulkan al dimensionar niveles.
4. **LOD (M-16, C3):** aceptar `.001` tras el sufijo y aplicar el filtro LOD también en los pases de sombra.
5. **UI (M-24):** recalcular descendientes tras aplicar layout groups (o resolver en dos pasadas).
6. **Spawn en vivo (C4):** decidir si la ruta en vivo resuelve mesh colliders con la geometría retenida (la caché vive en la app; el puente necesitaría acceso) o si se documenta como límite de la beta.
7. **Resto de mayores:** por fase, cuando se toque cada sistema.
