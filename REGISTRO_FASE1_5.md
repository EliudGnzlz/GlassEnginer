# REGISTRO — Fase 1.5: caja de herramientas de colliders

Cerrada el 2026-11-11. Decisión del usuario (2026-11-11, opción B): bloque
propio tras la Fase 1, sin tocar el orden del ciclo. Diseño:
`fase1/DISENO_FASE1_5.md` (fuera del repo). Expectativa tipo Unity: por
modelo, el usuario elige mesh collider (hull; trimesh solo estáticos) o
varios colliders primitivos compuestos por entidad.

## Entregado

### Mundo (glass_physics)
- `attach_collider_with_config_at(...)`: como el attach con config de la
  Fase 1 más desplazamiento local (la `translation` del builder de Rapier
  es relativa al cuerpo al insertar con parent).
- `attach_trimesh_collider(...)`: malla triangular exacta. **Validación
  previa obligatoria**: parry 0.18 no devuelve Err ante índices fuera de
  rango — indexa los vértices al reconstruir su QBVH y entra en pánico —
  así que el método rechaza con None las mallas vacías, con índices fuera
  de rango, con vértices no finitos o con todos los triángulos degenerados
  antes de llamar a Rapier.

### Modelo de escena (glass_core)
- `PhysicsData.colliders: Vec<ColliderSpec>` (serde default vacía = camino
  histórico de un collider inferido). `ColliderSpec { shape, size, offset,
  is_trigger }`: forma y tamaño por spec, desplazamiento local, trigger
  por collider; capa y máscara salen de la entidad.
- `PhysicsData.mesh_collider: MeshColliderMode` (None/Convex/TriMesh,
  serde default None).

### Geometría retenida (glass_renderer + app)
- `AssetCache.geometries`: posiciones e índices CPU de cada modelo
  importado, retenidos al cargarlo (los dos sitios de carga de la app,
  incluida la recarga en caliente) y expulsados junto a la malla GPU en
  `evict_idle`. Sin esta copia el `LoadedModel` se descarta tras la subida
  a GPU y ningún mesh collider sería posible.

### Cableado (glass_app)
- Una sola resolución (`colliders_resueltos`) usada por los dos caminos:
  spawn en vivo (`body_spec_for`/`create_body`, primitivos compuestos
  completos) y carga de escena (`rebuild_physics_from_scene`).
- En rebuild, el modo mesh se resuelve con la geometría retenida escalada
  por la escala de mundo: TriMesh solo en cuerpos Static; en no estáticos
  se degrada a Convex con aviso (regla Unity). Sin geometría (modelo
  integrado o no importado) cae a los primitivos con aviso.
- Límite conocido: el spawn en vivo no resuelve colliders de malla (el
  puente no tiene la caché); avisa en el log y usa los primitivos.

### Contenido
- `scripts/gen_canal_u.py` → `assets/models/canal_u.glb` (canal en U
  soldado: 72 vértices, 36 triángulos, cotas [-1.2,-0.2,-2]..[1.2,1,2]).
- `assets/scenes/Fase1_5Demo.scene.json`: el mismo canal como TriMesh y
  como Convex, una esfera para cada uno, y una entidad compuesta de dos
  cajas desplazadas.
- `docs/PHYSICS.md`: sección de la caja de herramientas.

## Verificación

### Unitaria
- 4 tests nuevos del mundo: offsets (dos esferas a ±1,2: el cuerpo reposa
  a la altura del radio y el rayo central pasa entre ellas), el canal en U
  como trimesh recibe la esfera en el fondo (y ≈ 0,4), el mismo canal
  como hull la deja sobre el borde (y ≈ 1,4), y trimesh degenerado →
  None sin pánico (este test cazó el pánico real de parry y motivó la
  validación previa).
- **Suite completa: 283 tests, 0 fallos**, con `-D warnings`.

### Test físico en la VM (escena demo, MCP en vivo)
- Reconstrucción: «6 cuerpo(s), 7 collider(s)» (la compuesta aporta 2).
- `EsferaTrimesh` reposa en **y = 0,3989** (dentro del canal, fondo+radio)
  y `EsferaConvex` en **y = 1,3987** (sobre el casco que cierra la boca):
  el mismo modelo, 1 m exacto de diferencia según el modo elegido.
- Compuesta reposa en **y = 0,2494** sobre sus dos cajas; el rayo por el
  hueco central (0,5,7) atraviesa y acierta el Suelo (distancia 5,0) y el
  rayo sobre la caja desplazada (1,5,7) acierta la Compuesta (distancia
  4,5006, punto y = 0,4994).
- Captura en `fase1/test-fisico15/` (fuera del repo): la esfera sobre el
  canal convexo y la del trimesh dentro del suyo.

## Incidencias de la fase (honestidad de registro)
- La primera versión de la escena demo no cargaba: mi especificación del
  paquete de contenido dictó el esquema con dos errores (la `layer` de
  entidad es la cadena "Default", no 0; CameraData usa
  near_clip/far_clip/is_main). La escena se corrigió contra el esquema
  real comparando con la demo de la Fase 1.
- La primera corrida en vivo dejó caer la entidad compuesta al vacío: el
  Suelo de mi escena llevaba `box_extents` [0.5,0.5,0.5] (un cubo de 1 m
  en el origen) porque para "cube" el box_extents ES el tamaño del
  collider, sin multiplicar por la escala (la escala solo deriva las
  formas nuevas sphere/capsule/cylinder). Corregido a [20, 0.5, 20].
- Una integración del cableado de rebuild se aplicó sobre un ancla no
  única y corrompió main.rs (~2.500 líneas sustituidas); se restauró el
  archivo desde el commit de la Fase 1 y se reaplicaron los dos paquetes
  con anclas verificadas por conteo y acotadas a su función. Sin pérdida:
  los paquetes generados estaban guardados fuera del repo.

## Consumo por proveedor (tokens)
- DeepSeek: 11.719 (DS-1.5a: métodos del mundo + tests).
- Claude: 21.887 (CL-1.5a 6.497: datos y geometría; CL-1.5b 7.747: puente;
  CL-1.5c 7.643: rebuild).
- Kimi: 7.806 (KM-1.5: generador del .glb, escena demo y docs).

## Siguiente
Fase 2 (render): frustum culling, compresión de texturas BC, sombras de
luces puntuales, niebla, y tools MCP `glass_render_stats` /
`glass_texture_info`; la librería de shaders cubre PBR y toon (la parte de
grafo es la Fase 2S).
