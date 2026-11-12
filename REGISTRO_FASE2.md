# Registro de la Fase 2 — Render: culling, compresión BC, sombras puntuales y niebla

**Cerrada el 2026-11-11** (verificación física completada la noche del 11 al 12). Suite del workspace: **299 tests, 0 fallos**, con `RUSTFLAGS="-D warnings"`. Tools MCP: 22 → **24**.

## Entregado

- **Frustum culling** en la app al construir las listas de dibujo (`render_frame`): AABB local de la geometría retenida (mapa `AssetCache.bounds`, rellenado en `insert_geometry`) transformada por la matriz de mundo contra los 6 planos del VP de cada vista (Gribb-Hartmann adaptado a la proyección Vulkan del motor, cerca = fila 2). Las listas de sombras NO se filtran y los draws de portal quedan fuera. Contadores en `glass_core::render_stats`.
- **Compresión BC de texturas**: el dispositivo activa `textureCompressionBC` si el hardware lo soporta (log «Compresión BC de texturas: activada/no soportada»). Con alfa → BC3 SRGB; sin alfa → BC1 SRGB (crate `texpresso` 2.0.2). Los formatos BC no admiten blit: los mips se generan en CPU (downsample de caja) y se suben nivel a nivel. La ruta RGBA8 queda intacta como respaldo. `GpuTexture` lleva ahora `format`, `mip_levels` y `vram_bytes`.
- **Sombras de luces puntuales**: `LightData.cast_shadow` (serde default false). Atlas compartido: imagen D32 cube-array de 4 cubos × 512² (24 capas) en el set 2 binding 2; se asignan hasta 4 ranuras por cercanía al ojo entre las luces que lo piden; comparación manual en el shader PBR y su gemelo legacy (muestreo normal + fórmula NDC, bias 0,0025). 6 pases de profundidad por luz con sombra, espejo del pase del sol.
- **Niebla analítica**: `FogData { color, near, far }` como bloque en una entidad (manda la primera que lo lleve); viaja en el UBO del set 2 (80 → 112 B) y se mezcla lineal por distancia en `pbr.frag` y `pbr_legacy.frag` tras la iluminación. El nodo de grafo de niebla queda para la Fase 2S, que rehace el sistema de grafos.
- **Tools MCP nuevas**: `glass_render_stats` (consideradas/descartadas/dibujadas, % de descarte y draw calls por vista y de sombras) y `glass_texture_info` (formato, dimensiones, mips y bytes por textura + totales, con filtro opcional por modelo). La app registra la caché de assets en un global al arrancar, espejo del mundo físico.
- **Contenido**: `docs/RENDER.md`; escena `Fase2Demo` (215 entidades, 205 con malla, niebla, 8 puntuales con 4 `cast_shadow`) generada por `scripts/gen_escena_fase2.py`; modelos `cubo_checker_opaco.glb` / `cubo_checker_alfa.glb` generados por `scripts/gen_cubos_textura.py` (el proyecto no tenía NINGÚN modelo con textura: 0 de todos los .glb); escena `Fase2Texturas` y escenas mínimas A/B de sombra puntual `Fase2SombraMin` / `Fase2SombraMinNo`.

## Test físico (VM, lavapipe) — `~/workspace/glass-engine/fase1/test-fisico2/`

- **Culling**: cámara mirando a −Z en Fase2Demo: considered 205, culled 118 (57,6 %), drawn 87, game_draw_calls 87. Las cifras cuadran con la geometría exacta: zona B trasera (72) + zona C cercana (46) descartadas; la C lejana (24) entra porque el frustum se ensancha con la distancia; A (54) + suelo + flotantes (8) + C lejana (24) = 87. `shadow_draw_calls` 205 (sin filtrar, por diseño).
- **BC en vivo**: «Compresión BC de texturas: activada» en el log. `glass_texture_info`: alfa → `BC3_SRGB_BLOCK` 256×256, 9 mips, 87.408 B; opaco → `BC1_RGBA_SRGB_BLOCK`, 43.704 B (la mitad; el RGBA con mips ocuparía ~349,5 KB). Captura `captura_texturas_ok.png`: damero magenta/blanco nítido en ambos cubos, decodificación correcta.
- **Niebla**: captura `captura_fase2_final.png` — las filas lejanas de la zona A se lavan hacia el color de niebla y el suelo se funde con el horizonte; las cercanas conservan el color.
- **Sombras puntuales**: escena mínima A/B (una puntual sobre un cubo flotante, sin sol): con `cast_shadow` aparece la mancha oscura bajo el cubo (media de gris 207,1 en la región bajo el cubo); sin ella, el suelo queda iluminado (229,9). Capturas `captura_min_con.png` / `captura_min_sin.png`.
- La orientación de caras del cubo quedó verificada de forma indirecta: la mancha cae bajo el occluder (dirección −Y correcta) sin artefactos ni espejos visibles.

## Incidencias (quedan como lección)

1. **Colisión de nombre de prompt entre fases**: `pKM2_kimi.txt` ya existía de la Fase 1 (documento de física); la escritura del prompt de la Fase 2 fue rechazada por la guarda de lectura previa y DOS proveedores (Kimi 4.003 y Claude 7.369 tokens) respondieron fielmente al prompt viejo. Regla: nombres de paquete con la fase incluida (`pKM2F2_…`).
2. **DS-2c (DeepSeek)**: 11.000 tokens de razonamiento y 0 de contenido; por la regla vigente no se reintentó igual: el mismo paquete lo resolvió Kimi como KM-2c (4.665).
3. **Escenas que no cargan en silencio**: el cargador cae a la escena por defecto sin error visible ante un fallo de parseo. La escena de la fase falló TRES veces por esquema, cazadas una a una: falta `layer: "Default"` en las entidades; faltan `wireframe` y `shader` en el bloque `mesh` (sin `#[serde(default)]` en `MeshData`); falta `mass` en el `physics` del suelo (idem en `PhysicsData`). El test `scene_assets` da el error exacto de serde: usarlo ANTES del test físico.
4. **El «triángulo fantasma»**: la escena de texturas mostraba solo el triángulo de prueba del renderer. Causa: sus entidades llevaban el componente `"Mesh"` en vez de `"MeshRenderer"` → lista de dibujo vacía → el renderer presenta su triángulo integrado y las estadísticas de render nunca se escriben (la nota «sin datos» de la tool fue la pista). La bisección (misma escena con modelos `cube`) aisló que no era la textura sino el esquema.
5. **Reemplazo de VM a mitad del test físico**: se perdieron `libxkbcommon-x11-0` y los drivers Vulkan; la app moría con pánico de `xkbcommon-dl`. Reinstalados por apt (mirror lento: ~9 min). Xvfb también hubo que relanzarlo.
6. Claude se negó en el primer intento de CL-2a por no ver el código real: los paquetes de modificación llevan fragmentos reales pegados en el prompt (ya era la regla; se reafirma).

## Consumo por paquete

- Claude: CL-2a 18.644 (dos intentos), CL-2b 7.409, CL-2c2 9.666, CL-2c1 28.110 → 63.829 útiles (+7.369 del prompt equivocado).
- DeepSeek: DS-2a 14.262, DS-2b 4.484 → 18.746 útiles (+12.274 quemados en DS-2c).
- Kimi: KM-2c 4.665, KM-2F2 5.209, KM-2b 5.545 → 15.419 útiles (+4.003 del prompt equivocado).
