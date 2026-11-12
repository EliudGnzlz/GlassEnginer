# Sistema de UI de GlassEngineR

## Resumen

La UI de juego es un **Canvas screen-space (overlay)** al estilo Unity. Se dibuja mediante un pase 2D propio sobre la escena 3D; no utiliza egui, que está reservado al editor.

Los elementos de UI son entidades de la escena. Sus datos se almacenan como componentes y se resuelven con la misma lógica para pintado, hit-test de botones y drag & drop.

## Componentes

- **Canvas**: define el espacio raíz de la UI y la configuración del escalado.
- **RectTransform**: describe posición, tamaño, anclas y pivote.
- **Text**: texto con fuente TTF de `assets/fonts`, además de fuente y alineación seleccionables por entidad.
- **Button**: botón con color de gráfico y etiqueta.
- **Image**: sprite y tinte, con preservación de proporción opcional mediante `preserve aspect`.
- **LayoutGroup**: organiza automáticamente los hijos UI directos.

Texto, imágenes, botones y rectángulos también son accionables desde scripts C# mediante las clases `Text`, `Image`, `Button` y `RectTransform`.

El sistema incluye drag & drop con casillas receptoras, pensado para interfaces como inventarios.

## RectTransform y anclas

`RectTransformData` contiene:

- `anchored_position`
- `size_delta`
- `anchor_min`
- `anchor_max`
- `pivot`

Se sigue la convención de Unity:

- Las anclas usan coordenadas normalizadas `0..1` relativas al rectángulo padre.
- `(0,0)` corresponde a la esquina inferior izquierda.
- El pivote es el punto del rectángulo que se coloca en la posición anclada.

Cuando `anchor_min == anchor_max`, el elemento usa su posición anclada y `size_delta` como tamaño.

Cuando `anchor_min != anchor_max`, el rectángulo se **estira con el padre**. Su tamaño es el espacio comprendido entre ambas anclas más `size_delta`. Por ejemplo, unas anclas `(0,1)-(1,1)` producen una banda superior que se estira horizontalmente.

La resolución vive en `glass_core`:

- `resolve_ui_rect_in` resuelve un elemento contra un rectángulo padre.
- `resolve_ui_rect` lo resuelve contra la pantalla.

Por compatibilidad histórica, el tamaño pintado se limita a un mínimo de **10 px**, pero la posición se calcula usando el tamaño original sin aplicar ese mínimo.

## Jerarquía

Cada elemento se resuelve contra el rectángulo de su **ancestro UI más cercano**, es decir, su padre en la jerarquía de entidades, no directamente contra la pantalla.

La entidad `Canvas` actúa como espacio raíz: su rectángulo representa la pantalla en unidades de referencia. Esto permite anidar elementos y mantener posiciones y estiramientos relativos a contenedores intermedios.

## Canvas Scaler

El `Canvas` declara:

- `reference_resolution`: resolución de referencia; por defecto, **1920×1080**.
- `match_width_or_height`: `0` prioriza el ancho, `1` prioriza el alto y `0,5` mezcla ambos.

El factor por ancho es el ancho de pantalla dividido entre el ancho de referencia; el factor por alto se calcula de forma equivalente. El factor final interpola **logarítmicamente** ambos factores según `match_width_or_height`, como «Scale With Screen Size» de Unity.

Equivalentemente, el factor final es el factor de ancho elevado a `1 - match`, multiplicado por el factor de alto elevado a `match`.

Todo el layout se calcula primero en unidades de referencia y se multiplica por el factor final al terminar. Así, la misma escena conserva sus proporciones a distintas resoluciones.

## Layout groups

Una entidad con `LayoutGroup` recoloca a sus hijos UI directos dentro de su rectángulo, respetando el orden de escena.

Parámetros:

- `direction`: horizontal o vertical.
- `spacing`: separación entre hijos.
- `padding`: márgenes `[izquierda, arriba, derecha, abajo]`.
- `alignment`: `start`, `center` o `end`.

Los hijos se disponen en fila o columna y **conservan su tamaño**. Esta versión no incluye Control Size.

Un ejemplo completo está en `assets/scenes/Fase5Layout.scene.json`: contiene una banda superior con anclas estiradas `(0,1)-(1,1)`, un título hijo de esa banda y un panel inferior estirado con un `LayoutGroup` horizontal centrado de tres botones.

## Inspección por MCP

- `glass_ui_tree`: devuelve el factor de escala y el rectángulo en píxeles de cada elemento para una resolución dada.
- `glass_ui_stats`: devuelve conteos y profundidad de la jerarquía de UI.

## Límites conocidos

- `Canvas` solo admite screen-space; no existe world-space.
- La previsualización del editor no compone la jerarquía ni aplica el Canvas Scaler. El runtime sí lo hace.
- Los `LayoutGroup` se configuran manualmente en JSON; todavía no tienen Inspector ni API C#.