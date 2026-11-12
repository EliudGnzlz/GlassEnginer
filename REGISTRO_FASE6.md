# REGISTRO — Fase 6: Bridge .blend, hash, instancias y LODs

**Cierre: 2026-11-12.** Diseño propio: `fase1/DISENO_FASE6.md`. Especificación: §Fase 6 + anexo §8 del consolidado.

**Verificación de cierre: 342 tests, 0 fallos** (`cargo test --workspace` con `RUSTFLAGS="-D warnings"`). Tools MCP: 39 → **41** (`glass_asset_validate`, `glass_asset_reload`).

## Hallazgo que reordenó la fase

El importador fusionaba iterando `document.meshes()` e **ignoraba los nodos y sus transformaciones**: un .blend con varios objetos colocados se importaba apilado en el origen. Los modelos de un solo objeto no lo notaban; instancias y LODs por objeto exigían recorrer los nodos primero. Con piel, el camino es otro y no se tocó.

## Entregado

- **Importador por nodos** (`glass_renderer::mesh`): sin piel, la fusión recorre los nodos de la escena aplicando su transformación global a posiciones y normales (inversa-transpuesta normalizada); las mallas sin nodo se fusionan sin transformar al final (compatibilidad). `LoadedModel.objects: Vec<ModelObject>` registra cada objeto (nombre de nodo, malla, global, rangos aportados). La caché guarda los objetos por modelo.
- **Instancias enlazadas**: una malla referenciada por varios nodos se fusiona una vez por nodo (horneada) y queda registrada: varios objetos con el mismo `mesh_name`. El sidecar las marca desde Blender con `instance_of`.
- **LODs por sufijo `_LOD<n>`**: `lod_suffix` + `lod_level_for_distance` (umbrales 20 m / 45 m; mayor disponible ≤ deseado, o el menor disponible). La app filtra los rangos LOD al construir los lotes según la distancia de la entidad al ojo; los rangos no-LOD se dibujan siempre.
- **Bridge v2** (`tools/headless_blender_bridge.py`): resuelve Blender por `GLASS_BLENDER` o PATH con error claro; inventaría la escena dentro de Blender (objetos, mallas compartidas, LODs, colecciones, `Prefab_<X>` → prefab); valida el GLB (cabecera + chunk JSON + conteos, sin mallas = error); escribe el **sidecar** `<modelo>.glb.meta.json` (fuente, SHA-256, fecha, versión de Blender, conteos, inventario); última línea `GLASS_BRIDGE_RESULT {json}`. Si el inventario no llega, la exportación se declara fallida y NO se pisa el sidecar (endurecido tras un caso real de inventario vacío).
- **Vigía con hash**: SHA-256 del .blend/.fbx antes de exportar (crate `sha2`); sin cambio de contenido no hay re-exportación (los guardados en ráfaga ya no disparan Blender); si falta el script del bridge, `log::error` (antes: silencio total).
- **MCP**: `glass_asset_validate` {path} — conteos del GLB, instancias (nodos que comparten malla), grupos LOD, sidecar con colecciones/prefabs y **frescura** por SHA-256; `glass_asset_reload` {path} — ejecuta el bridge en el acto para .blend/.fbx (la recarga en caliente llega por el vigía al escribirse el .glb); rutas fuera de `assets/` rechazadas.
- **Contenido**: `assets/models/fase6_test.blend` (generador en `fase1/gen_fase6_blend.py`) + su .glb y sidecar, escena `Fase6Lod`, y `docs/ASSETS.md` (Kimi).

## Test físico (VM; driver `fase1/driver_f6.py`, capturas en `test-fisico6/`)

- `glass_asset_validate` sobre el .blend: 5 mallas / 7 nodos; instancias = malla 0 compartida por CuboF6, CuboF6.001 y CuboF6.002; grupo LOD `EsferaF6: [0, 1, 2]`; sidecar presente con el prefab `Caja` y `fresh: true` (SHA-256 coincide).
- `glass_asset_reload` en vivo: `ok: true` con el mismo hash y el resumen completo (7 objetos, 3 instancias, LODs [0,1,2], prefab Caja). Re-verificado después en una segunda sesión.
- Captura `captura_f6_lod.png`: la entidad cercana muestra los objetos **en sus posiciones de Blender** (tres cubos-instancia, la **esfera detallada del LOD0** y la caja del prefab); la entidad a 58 m muestra la misma fila en miniatura con el **cubo del LOD2** en el lugar de la esfera.
- Regresión: Plat3D_L1 se renderiza íntegro con la fusión por nodos (plataformas, sombras y HUD en su sitio) y la suite completa pasa.

## Deudas anotadas

- El pase de sombras dibuja todos los niveles LOD de un modelo (la silueta es la unión; coste menor, corrección pendiente).
- Las instancias se hornean en el buffer fusionado: no hay dibujo instanciado por GPU (eso es trabajo de render futuro, no de esta fase).
- Las colecciones `Prefab_*` se registran en el sidecar y se reportan, pero aún no generan archivos `.prefab.json` automáticamente.

## Consumo (de los `out*.json` de la fase)

- **Claude: 17.940 tokens** (importador por nodos + objetos + helpers LOD; su P2 insertó las funciones dentro del `impl` y hubo que sacarlas a mano).
- **DeepSeek: 2.893 tokens** (vigía con hash, completo).
- **Kimi: 2.358 tokens** (docs/ASSETS.md).
- El bridge v2, las tools MCP, el contenido .blend y el filtro LOD del dibujo fueron integración de Rayo.
