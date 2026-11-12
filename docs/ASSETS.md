# Pipeline de assets 3D

## Resumen

GlassEnginer importa modelos fuente `.blend` y `.fbx` y los convierte a `.glb`, único formato cargado por el runtime. Un vigía sobre `assets/` automatiza la exportación y la recarga en caliente de mallas GPU, materiales, texturas y geometría CPU para colliders y piel.

## Flujo de trabajo

1. Guardar el modelo `.blend` o `.fbx` dentro de `assets/`.
2. El vigía ejecuta `tools/headless_blender_bridge.py`.
3. El bridge arranca Blender en modo headless y exporta el `.glb` junto al archivo fuente.
4. El bridge valida la cabecera, el chunk JSON y los conteos del GLB.
5. Al escribirse el `.glb`, el motor recarga el modelo en caliente.

El binario de Blender se resuelve mediante la variable de entorno `GLASS_BLENDER` o, en su defecto, mediante `PATH`.

Un `.glb` modificado por otra herramienta también provoca una recarga; no es necesario pasar por el bridge.

## Hash de contenido y sidecar

El vigía calcula el SHA-256 de la fuente y solo reexporta cuando su contenido cambia realmente. Los guardados en ráfaga de Blender sin modificaciones no generan trabajo adicional.

Si falta el script del bridge, el fallo se registra como error; nunca se ignora silenciosamente.

Cada exportación escribe `<modelo>.glb.meta.json` con:

- Fuente, SHA-256 y fecha de exportación.
- Versión de Blender.
- Conteos del GLB.
- Inventario de la escena: objetos con malla, colecciones, padre y posición.
- Colecciones `Prefab_<X>`, registradas como prefab `<X>`.

## Importador por nodos

El importador recorre los **nodos** del glTF y aplica su transformación global a la geometría. Cada nodo con malla queda registrado como objeto del modelo, con su transformación y sus rangos de índices.

Esto corrige la fusión histórica que ignoraba los nodos y apilaba en el origen archivos con varios objetos colocados. Los modelos con piel conservan esa fusión histórica: la piel compone las transformaciones en el shader.

## Instancias enlazadas

Cuando varios objetos de Blender comparten malla, el sidecar los marca con `instance_of`. El importador registra varios objetos con el mismo nombre de malla.

La geometría se hornea una vez por instancia en el buffer fusionado. Todavía no existe dibujo instanciado por GPU.

## LODs

Los objetos `<Base>_LOD0`, `<Base>_LOD1` y `<Base>_LOD2` forman un grupo. Al dibujar una entidad, solo se emite el rango correspondiente al nivel activo según su distancia a la cámara.

| Nivel | Distancia |
|---|---|
| LOD0 | Menos de 20 m |
| LOD1 | Hasta 45 m |
| LOD2 | Más allá |

Si falta el nivel deseado, se usa el mayor disponible por debajo; si no existe, el menor disponible. Los objetos sin sufijo LOD se dibujan siempre.

El pase de sombras aún dibuja todos los niveles; esta limitación está registrada como deuda.

## Inspección por MCP

- `glass_asset_validate`: informa de conteos, instancias, grupos LOD y sidecar. Comprueba la frescura comparando el SHA-256 actual de la fuente con el registrado en la exportación.
- `glass_asset_reload`: reexporta inmediatamente un `.blend` o `.fbx`. La recarga posterior llega mediante el vigía.

Activo de referencia: `assets/models/fase6_test.blend`, con un cubo y dos duplicados enlazados, una esfera en tres niveles LOD y la colección `Prefab_Caja`. La escena `assets/scenes/Fase6Lod.scene.json` coloca el modelo cerca, en LOD0, y lejos, en LOD2.

## Límites

- El runtime no carga directamente `.blend` ni `.fbx`.
- Las instancias enlazadas duplican geometría en el buffer fusionado; no usan instanciado GPU.
- Los modelos con piel no siguen el recorrido por nodos del importador general.
- El pase de sombras no aplica selección LOD.