# Informe de la Fase 10 — Demo FPS + prueba integral fundida

**Estado: FINAL.**

Entorno de todas las mediciones: VM con Vulkan por software (lavapipe),
build de desarrollo sin optimizar. Los tiempos absolutos NO son
representativos de una GPU real; lo que vale es la comparación entre
escalones dentro de la misma sesión y la corrección funcional.

## 1. La demo FPS (sesión del campo, completa)

Escena `Fase10CampoTiro` (402 entidades): puesto de tiro, dianas a
10/25/50/100 m, dos blancos móviles sobre raíles, tres barriles
explosivos encadenables, 24 proyectiles y la zona integral clonada.

Diario de la sesión (22 disparos programados, `[F10] SESION FIN`):

- **Diana 10 m**: agrupación cerrada de pistola y escopeta (8
  perdigones efectivos por disparo). Los impactos caen ~6 cm por
  encima del centro apuntado de forma sistemática (sesgo de pitch sin
  aislar; a 10 m es visible, a 100 m lo domina la caída).
- **Dianas 25 y 50 m**: impactadas tras corregir la autoría (en la
  primera sesión estaban colineales con la de 10 y se tapaban entre
  sí ante el raycast; ahora están desplazadas −4 m y +5 m en X).
- **Francotirador a 100 m**: 2 impactos en el panel con caída medida
  de **1,46/1,47 m** (la sesión 1 midió 1,44/1,48: reproducible). La
  predicción ingenua sin rozamiento da ~0,85 m; la diferencia la
  explica el amortiguamiento lineal de los cuerpos dinámicos, que
  alarga el vuelo. Mira con zoom: el puente `SetCameraFov` aplica
  62 → 18 → 62 (marcas de sesión y lectura del dato), y la captura
  con el **binario de juego puro** lo muestra en pantalla (ver el
  defecto siguiente).
- **Tunelización**: a 260 m/s el proyectil avanza ~4,3 m por paso
  físico y atraviesa los discos intermedios sin colisionar; el panel
  de 100 m es blanco solo de proyectiles por diseño (el hitscan lo
  intercepta la diana de 10, que está en la línea).
- **Barril explosivo**: golpeado (vida 20 → 8 tras 3 impactos de AR),
  **detonó y arrastró al barril vecino en cadena** (daño radial 25).
- **Blancos móviles**: impactados apuntando a su posición viva
  (3 impactos registrados entre ambos).
- **Pistola a 100 m**: 0 impactos en el panel — la caída vence al
  proyectil antes de llegar (pega en el suelo, que no registra).

Defecto de autoría encontrado por sondas MCP y corregido antes de la
sesión buena: en este motor el collider de escena sale de
`box_extents` en unidades de mundo y la escala del transform es solo
visual; el suelo clonado de la prueba integral dejaba huecos sin
física fuera de 34×26 m y los barriles se hundían. Regla registrada.

**Defecto encontrado al capturar la mira (localizado, arreglo
pendiente)**: en el binario **con editor** lanzado en `--standalone`,
la vista presentada sigue la posición y la rotación de la cámara de
juego pero **no su FOV**: durante toda la ventana de la mira
(frames 1500–1760 de la sesión, con `fov=18` ya aplicado al dato) la
proyección presentada no cambia (medido sobre un objeto estático:
×1,1 de tamaño aparente frente al ×3,8 esperado). La misma sesión,
la misma escena y el mismo guion con el **binario de juego puro**
(`--no-default-features`) muestran el zoom correctamente en
pantalla. El puente y el dato son correctos; la divergencia está en
la ruta de presentación del binario con editor (la que también usa
la vista de juego del editor), y su arreglo encaja en la sesión del
editor, el siguiente bloque de la cola. La mira, como sistema del
juego, queda verificada en el binario que se distribuye.

Incidente: la primera sesión correctiva murió a los ~17 s por un
congelamiento total de la app (deadlock entre el hilo MCP y el
principal, sin aislar); el driver se endureció con timeout de 90 s
por llamada MCP y la sesión buena corrió completa a la primera.

## 2. Escalones de luces (las dos series)

GPU render medio por frame (240 muestras por escalón), misma escena.
Serie A: build normal (tope duro en 8; las luces 9–16 no alumbran).
Serie B: rama temporal forzada a 16 (las 16 alumbran de verdad;
parche revertido tras medir, no forma parte del motor).

| Luces pedidas | A: GPU avg | B: GPU avg | B: total avg |
|---|---|---|---|
| 0 | 138,5 ms | 131,3 ms | 275,3 ms |
| 4 | 152,2 ms | 150,7 ms | 304,7 ms |
| 8 | 200,0 ms | 198,9 ms | 358,3 ms |
| 12 | 210,3 ms | 214,6 ms | 376,9 ms |
| 16 | 234,1 ms | 251,6 ms | 408,5 ms |
| 16 de noche (peor caso) | — | 285,0 ms | 444,3 ms |

Física plana (~12 ms) en todos los escalones de ambas series. Las
series coinciden en 0/4/8 (ruido ±5 %): la medición es consistente.
Incluso capadas, las luces 9–16 cuestan en la serie A (las entidades
existen, se dibujan y entran en la selección por cercanía).

El coste de alumbrar de verdad, en render por software: pasar de 8 a
16 luces reales suma **+52,7 ms de GPU (+26 %)** en esta escena, unos
+6,6 ms por luz extra. En una GPU real el bucle de luces cuesta mucho
menos en relativo; esto es el suelo pesimista, no el esperado.

## 3. Prueba integral fundida (misma sesión)

- Carga de la escena: **4,4 s**. Sesión total: 3.646 s.
- Memoria: RSS del proceso ~239 MB al inicio y ~241 MB tras el soak
  de 8 minutos — sin crecimiento. Pico: 325 MB.
- Partículas: 2.067 vivas al inicio → 2.072 al final (estable).
- Cola de comandos: 5 ciclos × (crear 200 + destruir 200) vía MCP,
  todos OK, sin fuga (spawn/destroy contados: 10).
- Consultas físicas de la sesión 1 de la integral: rayo/esfera/
  overlap 31/31 cada una.
- Eventos: 15 `collision_enter`, 1 `trigger_enter` en el diario.
- Día/noche en tiempo real: ciclo completo con medianoche y mediodía
  registrados; la zona funciona de noche con las luces de la escena.
- **Recarga del .blend en caliente: 3/3 OK** (mini-sesión propia:
  `glass_asset_reload` sobre `fase6_test.blend`, exportación
  determinista — mismo SHA-256 y mismos conteos en las 3: 5 mallas,
  7 nodos, LOD 0/1/2, prefab «Caja»). La sesión del campo la había
  fallado por el driver (sin la ruta de Blender), y al repetirla
  apareció un **defecto real del motor**: la cadena de re-exportación
  resolvía las rutas relativas contra la variable de entorno `$PWD`
  del lanzador en vez de contra la raíz del proyecto — si la app se
  lanza con un `$PWD` que no es la raíz (un acceso directo, un
  servicio, un IDE), la recarga del .blend fallaba con «Cannot read
  file» mientras el resto del motor funcionaba. La app en sí tenía
  el CWD correcto (verificado en /proc).
  **Arreglado en la propia fase** (orden expresa: los defectos de la
  Fase 10 se arreglan, no se anotan): las rutas de assets se
  resuelven ahora contra la raíz del proyecto en las tres capas de
  la cadena — el handler MCP `glass_asset_reload` pasa rutas
  absolutas ancladas en el VFS (`AssetVfs::locate`), el vigía
  canoniza su carpeta y deriva el bridge de la raíz, y el propio
  bridge ancla cualquier ruta relativa a la raíz deducida de su
  ubicación en `tools/`. **Prueba física del arreglo**: la app se
  lanzó con un entorno hostil (`PWD=/tmp`, simulando el acceso
  directo/IDE) y la recarga funcionó (ok, 5 mallas, 7 nodos, LOD
  0/1/2, prefab «Caja») — el mismo escenario que antes fallaba. Los
  campos `glb`/`sidecar` de la respuesta de la tool llevan ahora la
  ruta absoluta, que es lo correcto para un resultado.

## 4. Prueba estrella: el RPG contra los cúmulos de cubos

### 1.000 cubos (sesión completa, 1.670 s)

- Carga: **5,4 s**; cuerpos/colliders: **1.007/1.007**.
- La pila compacta no llega a dormirse en la ventana medida: la
  física se estabiliza en **~530 ms por paso** (desde 157 ms con la
  pila recién caída) y el frame total en ~1.040 ms. GPU render:
  ~360 ms (1.000 draws en render por software).
- **Explosión del RPG**: punto (0,01, 0,41, 1,96), radio 9 m.
- Memoria: 344–355 MB con la pila compacta; 255 MB al final.

### Defecto encontrado por la prueba y corregido: OverlapSphere truncaba a 64

La primera explosión registró **afectados=64** — exactamente 64. La
causa: el puente C# de `Physics.OverlapSphere` (de la auditoría C5)
usaba un buffer fijo de **64** entidades y truncaba en silencio; el
mundo Rust devuelve todas las intersecciones. En una pila de 1.000
cubos dentro del radio, la explosión solo empujaba a 64.

**Arreglo** (en `RuntimeBridge.OverlapSphere`): el buffer crece ×4
mientras la consulta lo llene (256 → … → 65.536). **Verificado**
repitiendo la sesión: la misma explosión registró **afectados=1.002**
(toda la pila dentro del radio), y la física posterior se estabilizó
en ~121 ms por paso con el cúmulo ya disperso (frame total ~326 ms).

### 5.000 cubos (sesión cosechada: asiento, disparo y explosión)

- Carga: **5,6 s**; cuerpos/colliders: **5.007/5.007**.
- Asiento de la pila compacta: frame total **~3.823 ms**, física
  **~1.666 ms por paso** y subiendo al compactar (1.376 → 1.666 en
  las muestras), RSS **584 MB**.
- El programa de disparos se adelantó por variables de entorno
  (equipar f180, disparo f240): al ritmo de la pila, el programa
  por defecto no llegaba al primer disparo en la ventana del
  driver.
- **Explosión del RPG**: afectados **5.002** (toda la pila dentro
  del radio de 9 m, con el arreglo de OverlapSphere activo).
- Tras la explosión la física queda en **~5.948 ms por paso**
  sostenidos (frame total ~7.700 ms): los cinco mil cubos lanzados
  siguen en el aire, cayendo y re-colisionando durante toda la
  ventana medida; RSS estable ~568 MB, sin crecimiento.

### 10.000 cubos (sesión cosechada: asiento, disparo y explosión)

- Carga: **5,4 s**; cuerpos/colliders: **10.007/10.007**.
- Asiento de la pila compacta: frame total **~7.881 ms**, física
  **~4.263 ms por paso**, RSS **654 MB**.
- Programa adelantado por entorno (equipar f60, disparo f120).
- **Explosión del RPG**: afectados **9.382** — no son 10.002
  porque la pila de diez mil crece hacia arriba y los cubos de la
  coronación quedan a más de 9 m del punto de explosión; es el
  conteo geométrico real de la esfera, sin truncar (con el defecto
  viejo habrían sido 64).
- Tras la explosión: física **~11.252 ms por paso** sostenidos
  (frame total ~14.035 ms), RSS ~841 MB estable. La sesión se
  cosechó tras la serie post-explosión; el segundo disparo y el
  cierre formal quedaban a ~1 h más a ese ritmo y no aportaban
  señal nueva.

**Lectura de la escalera completa**: el coste por cuerpo crece de
forma superlineal con la pila despierta (~0,53 ms/paso por cuerpo
en reposo a 1.000 → ~1,67 a 5.000 → ~4,26 a 10.000, en pasos por
milisegundo de la pila completa) y la explosión que despierta a
miles de cuerpos multiplica el paso por ~3-7× mientras el cúmulo
se dispersa. Son cifras de una VM por software con build de
desarrollo: marcan el orden de magnitud y dónde está el coste
(física de contactos de la pila), no el rendimiento en hardware
real. El motor no pierde memoria en ningún escalón (RSS estable
tras la dispersión) y la carga de escena es plana (~5,5 s).

## 5. Recomendación sobre el tope de luces

Mantener **8 como tope por defecto de la beta**: el presupuesto de
sombras (4 cubos) y el UBO están construidos alrededor de él, y en
render por software 16 reales cuestan +26 % de GPU. Pero el dato
importante es que 16 **no es un precipicio**: el coste es lineal y
modesto por luz. La respuesta de fondo a las escenas con muchas
luces no es subir el tope global, sino la asignación por objeto con
histéresis que entra en el bloque de integración de iluminación
siguiente: que cada objeto pague solo las luces que de verdad lo
tocan.
