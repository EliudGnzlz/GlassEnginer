# Inventario de secciones del motor — barrido por secciones

**Fecha:** 2026-11-25 · **Origen:** orden de D3m3tr10 — el motor se prueba **por secciones**, no en una corrida completa, y el barrido queda como **puerta permanente** de cierre para los bloques futuros. **Autor:** chat «Motor gráfico — Análisis» (solo lectura). **Para:** el chat del motor, que implementa el barrido como puerta y lo corre.

**Cómo se usa este documento:** cada sección tiene su ficha — qué cubre, **cómo se prueba por el camino real con evidencia medible** (la regla del test físico: durar no es testear; se inspecciona el estado real — valores en el Inspector, archivo de escena guardado, comportamiento medido con números y logs — y se declara qué se verificó de cada cosa) y **qué notas abiertas arrastra**. El §3 es el mapa de dependencias (la regla anti-regresión hecha tabla) y el §4 la regla de cierre permanente propuesta.

**Orden:** el de construcción real, verificado contra los registros de fases y bloques.

---

## 1. Las secciones, en orden de construcción

### S1 · Fundamentos: escena, entidades y transforms
- **Cubre:** `SceneState` y serialización de escenas (JSON), jerarquía padre/hijo, `TransformStore` SoA con orden topológico (Fase 7), spawn/despawn masivo sin fugas (escalera de esferas), guardado/carga y migración de formato.
- **Prueba:** crear jerarquía de 3 niveles, guardar, y **leer el archivo de escena**: ids, padres y transforms coinciden con lo puesto en el Inspector. Escalera corta medida: spawn/despawn de 10k esferas ×3 ciclos con pico y **residuo de RAM** anotados (un escalón solo vale repetido). Carga de una escena legada: abre y juega sin migrar mal.
- **Notas abiertas:** si el nombre registrado de la escena no coincide con el activo, «Save Scene» escribe por la clave del SceneManager y no en el archivo cargado (preexistente, registrado).

### S2 · Scripting C# y API estilo Unity
- **Cubre:** CoreCLR/.NET 8 embebido, fachada `UnityEngine` propia (GameObject, Transform, Rigidbody, Collider, Physics, Animator, AudioSource, Input…), ciclo de vida (Awake/Start/Update), parámetros de script.
- **Prueba:** un script de prueba en escena que mueva una entidad y registre por Consola: valores finales en el Inspector == los del script; error de compilación de un script → la Consola lo muestra como ERROR y el editor no muere. Sin .NET 8 instalado: la banda roja y el aviso aparecen (camino del Bug D).
- **Notas abiertas:** hot reload de C# no existe (registrado, post-beta).

### S3 · MCP base y Command Queue
- **Cubre:** servidor MCP nativo (stdio y sobre escena viva), Command Queue determinista de la Fase 8 (las mutantes se ejecutan en orden al inicio del frame), `glass_queue_submit/status`.
- **Prueba:** `tools/list` devuelve las 58; una mutante encolada se aplica en el frame siguiente y `glass_queue_status` lo refleja; dos mutantes en orden no se cruzan (crear → modificar la misma entidad, estado final correcto en la jerarquía).
- **Notas abiertas:** la completitud de tools es la sección S23; `glass_import_unitypackage` no está declarada mutante (arreglo anotado en la auditoría MCP).

### S4 · Física base y colliders originales
- **Cubre:** Rapier, capas/máscaras/triggers, eventos de contacto a C#, consultas (ray/sphere/capsule/overlap), caja de herramientas de colliders de la Fase 1.5, CharacterController.
- **Prueba:** la medida canónica del motor: **caída a 100 m = 1,46 m** (la cifra de la Fase 10, reproducible); un trigger que registra entrada/salida en log con conteo; raycast contra una pared con distancia medida == distancia colocada.
- **Notas abiertas:** la paridad fina con Unity es S16 y los colliders como componentes son S19 (esta sección es la base histórica).

### S5 · Render base y materiales
- **Cubre:** Vulkan/PBR, culling por frustum, texturas BC al cargar, sombras (puntuales en atlas y de sol con PCF), niebla, post HDR (ACES, bloom, viñeta), ciclo día/noche.
- **Prueba:** escena fixture con materiales conocidos: **píxeles medidos** de una captura (color del suelo y de dos materiales contra sus valores esperados, como se cerró el piso blanco: textura (35,37,49) → píxel (22,28,34)); `glass_render_stats` y draw stats coherentes con el número de objetos de la fixture.
- **Notas abiertas:** **el factor de iluminación/contraste tipo URP** — tras cerrar el piso blanco quedó una diferencia de look frente a Unity (materiales correctos, contraste/luz distintos) atribuida a la **futura parte gráfica**: esta sección la arrastra como nota hasta que la parte gráfica la recoja. Sin compresor de texturas propio (solo BC al cargar glTF): también de la parte gráfica (porte nº 9).

### S6 · Shaders (rediseño 2S)
- **Cubre:** grafo de shaders propio → GLSL → SPIR-V, propiedades y UBO por instancia, pases Shadow/Outline, librería «tipo brocha» y presets toon, fallback magenta, recarga en caliente.
- **Prueba:** aplicar un preset a un material y medir el píxel resultante en captura; shader con error → fallback magenta visible y error en Consola, sin cierre; `glass_shader_compile_check` y `glass_reload_shaders` sobre la escena viva.
- **Notas abiertas:** por orden de D3m3tr10 los shaders van **después de la parte gráfica** (diseño pendiente de su OK): el cableado por arrastre del grafo y la paridad con Shader Graph quedan para ese bloque.

### S7 · Iluminación
- **Cubre:** perfiles de luz por objeto, asignación de luces, IBL básica de skybox, spot emisiva, bake de lightmaps v1.
- **Prueba:** fixture con dos luces y un objeto con perfil: `glass_light_assignment` dice la asignación esperada y la captura la confirma en píxeles; bake de una escena pequeña → el lightmap se genera y la escena horneada se ve igual con las luces dinámicas apagadas (medido, no a ojo).
- **Notas abiertas:** comparte con S5 la nota del factor tipo URP (la parte gráfica decidirá el modelo de luz definitivo).

### S8 · Audio (Fase 3)
- **Cubre:** buses Master/Music/SFX, ducking, low-pass por oclusión, fuentes 2D/3D.
- **Prueba:** `glass_get_audio_status` y `glass_audio_buses` con valores puestos y leídos; ducking medido: al sonar SFX, el bus Music baja los dB fijados y vuelve; oclusión: fuente tras una pared suena filtrada (estado del filtro en el status).
- **Notas abiertas:** ninguna propia; el control por fuente por MCP está en la lista de S23.

### S9 · Animación (runtime)
- **Cubre:** Animator con controladores JSON, AnyState, parámetros, condiciones + exit time + cross-fade, BlendTree 1D/2D, root motion, eventos a C#, skinning con slerp.
- **Prueba:** personaje fixture con dos estados: `glass_animator_state` refleja el estado tras fijar el parámetro con `glass_animator_set_param`; un evento de clip llega a C# (log); root motion medido: desplazamiento tras N segundos == el del clip ± tolerancia.
- **Notas abiertas:** el **autorado es cero** (sin sección de Animator en el Inspector, sin editor de clips): es el expediente de animación del 2026-11-24, post-1.0, con el auto-rigging como pieza 1,5 de su mapa. No es un defecto de esta sección; es su frontera conocida.

### S10 · UI/Canvas clásica
- **Cubre:** Canvas (Scaler fusionado), RectTransform con anclas y estirado, Texto/Botón/Imagen, LayoutGroup H/V, drag & drop en partida, pestaña «🖼 Canvas / UI» (P10).
- **Prueba:** construir una UI por el editor y por MCP (`glass_ui_tree`): jerarquía y textos coinciden; botón que responde en Play (log al clic); ancla en esquina: al cambiar la resolución, el elemento queda donde la ancla manda (posición medida en captura).
- **Notas abiertas:** los modos **Camera/World del Canvas son etiqueta sin implementar** (solo Overlay corre) — frontera conocida que recoge S21. Sin ScrollRect/Toggle/Slider/InputField (familia nunca construida; va en el expediente de la fase UI).

### S11 · Assets e importación base (glTF, .blend, FBX)
- **Cubre:** carga de glTF, puente con Blender (headless → .glb, recarga en caliente, LOD por sufijo), FBX nativo (`glass_fbx`, ufbx, con fallback a Blender), AssetWatcher, VFS y resolución de rutas contra el proyecto abierto.
- **Prueba:** arrastrar un .blend y un .fbx conocidos: conversión terminada, modelo en escena con su **número de mallas/materiales** contados, y su collider generado; tocar el .blend en disco → recarga en caliente sin reiniciar; abrir el mismo proyecto lanzando el editor **desde otra carpeta**: los materiales se resuelven igual (el defecto de rutas del 2026-11-25 no vuelve).
- **Notas abiertas:** Blender portable desde el Hub (descarga ofrecida si no hay Blender) sin probar en el equipo de D3m3tr10 — de su lado, con la prueba de Linux (S15).

### S12 · Partículas (Fase 8P)
- **Cubre:** sistema CPU SoA, emisores, billboards y stretched, API C# y tools MCP.
- **Prueba:** emisor fixture: `glass_particle_stats` cuenta las partículas vivas esperadas tras `glass_particle_emit`; captura con el emisor visible; pool sin crecimiento tras emitir/morir ×10 (memoria estable en `glass_memory_stats`).
- **Notas abiertas:** backend GPU de partículas: fuera (post-1.0, registrado).

### S13 · Portales y escena escaparate (Fase 9)
- **Cubre:** portal en vivo (cámara virtual, recorte oblicuo, cruce con impulso), la escena escaparate (avenida PBR/toon, lluvia, tormenta de arena).
- **Prueba:** cruce del portal medido: el personaje entra a X m/s y sale conservando el impulso (**7,1 m/s** en el cierre original); `glass_list_portals` lista el par; la escena escaparate carga sin errores en Consola.
- **Notas abiertas:** el pulido visual de las demos está pendiente por diseño (su función hoy es ejercitar sistemas).

### S14 · Editor: sesión, undo, Inspector y Gizmos
- **Cubre:** paneles y docking con layouts, undo global por instantáneas, Inspector estilo Unity, gizmos como Unity, selección con contorno por casco invertido en blanco, contorno de collider solo al editarlo, Consola.
- **Prueba:** editar un valor en el Inspector → el archivo de escena guardado lo trae; Ctrl+Z con la combinación mantenida (la lección de los clics sintéticos) deshace y el Inspector lo refleja; contorno de selección visible y **medido en captura**; el contorno del collider NO aparece por seleccionar y SÍ al expandir el componente.
- **Notas abiertas:** **Defecto 1 (el tacto de la navegación): arreglado, en revisión personal de D3m3tr10** — no se cierra sin su ratón. Las faltas de la auditoría de UI del editor (multiselección, overlays, candado de Inspector, vista ortográfica…) son un frente registrado con su ranking, no defectos sueltos.

### S15 · Hub/Launcher y distribución
- **Cubre:** Hub offline estilo Unity 2019.3, división Hub/motor (payloads separados), updates con verificación SHA-256 (**nada se actualiza solo**: se ofrece y el usuario instala), dependencias (.NET 8 comprobado por el Hub), instalador Windows, AppImage y zips de Linux, releases con limpieza (última + una anterior).
- **Prueba:** con una release nueva publicada: el Hub la **ofrece** sin instalarla; al aceptar, descarga, verifica el hash y la versión en uso cambia (leído en `engines.json` y en el pie del Hub); proyecto creado desde el Hub abre en el editor.
- **Notas abiertas:** **la prueba de descarga/actualización en Linux desde el Hub sigue sin probarse del lado de D3m3tr10** (bloqueada: convertidor VGA→DisplayPort dañado, sin pantalla hasta reemplazarlo; y «bajar la AppImage actual y abrirla una vez» sigue en su lista). Del lado del probador, su Linux tampoco ha corroborado la actualización (único punto vivo de su lista de bugs).

### S16 · Paridad RB/CC al 100% y multinúcleo
- **Cubre:** la paridad de Rigidbody y CharacterController contra Unity (RB 56/57 + CC 25/25 tras CCX), física con pool paralelo (feature `parallel`), partículas y transforms en paralelo.
- **Prueba:** la caída de 100 m sigue en **1,46 m** con el solver paralelo (la verificación que fijó el bloque); un cuerpo con Freeze por ejes rota solo en el eje libre (medido); `excludeLayers` del CC probado contra una capa excluida: no colisiona.
- **Notas abiertas:** el único parcial histórico (la carga Collision sin `contacts[]`) quedó registrado como tal.

### S17 · Demo FPS y prueba integral (consumidores)
- **Cubre:** la Demo FPS (dianas a 10/25/50/100 m, armas por datos: calibre→caída, escopeta con dispersión, AR con alcance, mira por FOV, explosiones con impulso), la Demo FPS Polygon y Plat3D como juegos que consumen el motor.
- **Prueba:** diana a 25 m alcanzada con la caída del calibre calculada (impacto registrado); explosión que desplaza un cuerpo a la distancia medida; Plat3D arranca y su menú responde (la lección del «ningún control funcionaba»: era no salir del menú — la prueba entra en partida de verdad).
- **Notas abiertas:** ajuste v2 de la Demo FPS Polygon pendiente (en piezas cube, escala = box_extents).

### S18 · MeshForge R1/R2 — **EN PAUSA**
- **Cubre:** edición de malla en la escena (modelo ProBuilder), receta reproducible con historial, «Hornear y salir» asignando el modelo editado, entrada agnóstica al formato (se edita el .glb convertido).
- **Prueba (al reanudar y como no-regresión si se toca su cimiento):** editar un cubo (mover una cara una distancia fijada), hornear, y medir la malla resultante en el archivo guardado; reentrar: la receta reproduce la misma malla.
- **Notas abiertas (las dos acotaciones de D3m3tr10):** la presentación debe rehacerse a **barra vertical de iconos dentro de la vista de Escena** (tipo ProBuilder), y al editar un modelo de varias mallas debe **ofrecerse la lista de objetos por nombre** (hoy escoge uno al azar). Cola al reanudar: R4 → R3 → R5–R12.

### S19 · Colliders como componentes (bloques C y E) — CERRADO
- **Cubre:** `ColliderSpec` como componente, asset `.physmat.json` compartido (defecto del proyecto 0,7/0,05 Average; asset nuevo 0,6/0,6/0), `direction` de cápsula, legado como `Option` con fallback, refresco en caliente, C# `PhysicsMaterial` + alias `PhysicMaterial`.
- **Prueba:** dos entidades con el mismo asset: editar el asset cambia el rebote de **las dos** (rebote medido con damping 1,5: el dinámico por defecto); una escena legada abre y juega con su 0,1/0,6 **sin materializar** el spec virtual; el cruce 0,7/0,05 corre **recto** en Rapier (la corrección del 2026-11-24).
- **Notas abiertas:** el physmat no carga al arrancar si el editor se lanza desde la raíz del repo (preexistente, registrado).

### S20 · Importación Unity U0–U5 + endurecimiento — CERRADO, frente en pausa
- **Cubre:** lector de `.unitypackage`, registro de GUIDs, conversión de texturas/materiales/modelos FBX, convertidor de C# con garantía de compilación y cuarentena en `Incompatibles/`, escenas y prefabs convertidos, driver completo + acción en el editor + `glass_import_unitypackage`.
- **Prueba:** el test limpio del frente: **borrar todo lo importado y reimportar desde el `.unitypackage`** (NeoFPS: 4.417 assets) — conteos del informe contra los de referencia (1.192 convertidos · 656 copiados · 93 en cuarentena, todos por corrutinas), la carpeta `Incompatibles/` con **cero** incompatibles fuera de ella, y el FiringRange con su suelo oscuro medido en píxeles (S5).
- **Notas abiertas:** la cuarentena por **corrutinas** es el límite v1 conocido (93 scripts de NeoFPS); el frente quedó **pausado en punto limpio** por orden del 2026-11-25 (la release v0.10.25.2-beta quedó sin publicar: al retomar, rehacer bump+tag sobre el HEAD). Los prefabs que importa son copias aplanadas: su vínculo vivo es S22/Fase B.

### S21 · UI/Canvas V1 — **EN PAUSA**
- **Cubre (lo construido):** crate `glass_ui` (documento, estilos, derivación), schema de escena v1 con migración automática y copia, modos del Canvas como documento-sobre-textura en diseño.
- **Prueba (al reanudar y como no-regresión):** el fixture **EscenaMigracion**: una escena con la UI clásica abre migrada, la copia de seguridad existe, y la UI se ve y responde igual que antes de migrar (captura + clic medido).
- **Notas abiertas:** fase pausada por orden del 2026-11-25 para cerrar primero el importador; al retomar faltan el test físico del fixture y el informe de cierre de V1 con su consumo (Opus 63.626 + DeepSeek 19.516 ya gastados en la implementación).

### S22 · Prefabs vivos — **ACTIVO (Fase A cerrada, Fase B en curso)**
- **Cubre:** el plan de construcción de prefabs: vínculo vivo (`prefab_source` + `prefab_hash` + `prefab_local_id`, Fase A ✓), importador vinculado + Added/Removed + instancias legadas (Fase B, en curso), editor + MCP `glass_prefab_*` (C), overrides como datos (D), variantes y anidados vivos (E).
- **Prueba (por fase, la del plan):** A — dos instancias del mismo prefab, una tocada y una no: editar el asset, recargar, la intacta se actualiza **conservando sus ids de escena** y la raíz no se mueve de sitio; la tocada queda marcada y no se pisa. B — reimportación limpia de NeoFPS: las instancias llegan vinculadas y con sus diferencias Added/Removed aplicadas (conteo contra informe).
- **Notas abiertas:** Fase B en curso al escribir esto; C–E pendientes por orden. Fuera con motivo (del expediente): Prefab Mode, Model Prefabs y el horneado en build.

### S23 · MCP — completitud (auditoría entregada, construcción desplazada)
- **Cubre:** las 58 tools vivas y la lista anotada de la auditoría MCP del 2026-11-25 (14 ALTA + el arreglo de clasificación, 17 MEDIA, 3 de UI anotadas).
- **Prueba (cuando se construya):** por tool nueva, llamada real contra la escena viva con su efecto medido (la tool de componentes lee y escribe un Collider y el Inspector lo confirma; `glass_play_control` entra y sale de Play con el estado leído de vuelta).
- **Notas abiertas:** construcción **desplazada por el bloque de prefabs** (orden del 2026-11-25); las tools `glass_prefab_*` sí entran, pero por la Fase C de S22.

---

## 2. Lo que NO es una sección (para que el barrido no lo busque)

- **Terreno y NavMesh:** no existen en el motor. **IK, capas de animación y blend shapes:** no existen (expediente de animación). **Modo 2D:** no existirá (decisión suya); la vista ortográfica es de S14. **Online:** es herramienta/complemento futuro, no sección del motor hoy. **Auto-rigging:** bloque propio futuro (pieza 1,5 del mapa de animación). Nada de esto entra en el barrido hasta que nazca como bloque — y al nacer, entra por la puerta del §4.

## 3. Mapa de dependencias — qué se re-corre cuando un bloque toca un cimiento

| Si el bloque toca… | Se re-corren (además de su sección dueña) |
|---|---|
| **Formato de escena / serialización (S1)** | **BARRIDO COMPLETO** — es el cimiento de todas; incluye cargar una escena legada en cada sección con fixture de escena |
| TransformStore / jerarquía (S1) | S9 (skinning y root motion), S13 (portales), S17 (FPS), S22 (re-expansión) |
| Física: Rapier, colliders, materiales físicos (S4/S16/S19) | S16, S19, S17, S22 (instancias con física), S13 |
| Render / materiales / shaders (S5/S6) | S7 (luz sobre materiales), S13, S17, S18 (mallas editadas), S20 (materiales importados, con la medición de píxeles) |
| Assets / VFS / rutas de proyecto (S11) | S15 (updates sobre proyectos), S18, S20, S22 (el vínculo es por ruta) |
| Scripting C# / fachada (S2) | S9 (eventos de animación), S10 (UI por script), S17, S20 (convertidor de scripts) |
| MCP / Command Queue (S3) | S23 y la sección dueña de cada tool tocada |
| Editor: paneles, undo, selección (S14) | Las secciones con autorado que el bloque use: S18, S19, S21, S22 según el caso |
| Hub / updates / payloads (S15) | S1 (compatibilidad: el motor nuevo lee escenas de proyectos viejos), S11 |
| Prefabs: formato del asset o de la instancia (S22) | S20 (el importador produce instancias), S1 (serialización), S14 (Inspector de instancia) |

Regla de lectura: la fila manda sobre la memoria. Si un bloque toca dos cimientos, se re-corre la unión de las filas.

## 4. Regla de cierre permanente (propuesta de puerta)

1. **Al abrirse, todo bloque declara** su sección dueña y las vecinas que le tocan según el mapa del §3. La declaración va en la primera línea de su plan o informe.
2. **Al cerrar, el bloque corre:** la suite y el pipeline exacto de CI (regla ya vigente), **más el barrido físico de su sección y de las vecinas declaradas**, usando las fichas del §1 — escena y puntos de medición preparados antes, evidencia con números, y declaración de qué se verificó en cada sección corrida.
3. **Si el bloque toca el formato de escena, el cierre exige el barrido completo** por secciones (fila 1 del mapa), sin excepción por tamaño del bloque.
4. **Las secciones en pausa no se corren** salvo que el bloque toque su cimiento; entonces se corre su **prueba mínima de no-regresión** (la de su ficha, recortada a lo que el cimiento afecta).
5. **El informe de cierre anota el barrido por sección:** pasa / falla / no corrida con motivo. Una vecina en rojo frena el cierre igual que un CI en rojo: se arregla antes de apilar nada encima.
6. **Toda sección nueva nace con su ficha en este inventario antes de construirse** (qué cubre, cómo se prueba con evidencia, notas) — así la parte gráfica (que recogerá la nota del factor tipo URP de S5/S7), la UI V1 al reanudarse y cualquier bloque futuro entran por la misma puerta, y este documento crece por el final, en orden de construcción.

**Consumo del inventario:** DeepSeek 0 · Kimi 0 · Claude (APIs) 0 — verificación de Rayo contra los registros de fases, los expedientes del 2026-11-24/25 y la memoria del proyecto.
