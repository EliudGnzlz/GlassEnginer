# Diseño final — Integración de iluminación (síntesis de Rayo)

Base: `INTEGRACION_ILUMINACION.md` (orden) + diseño en dos paquetes
de Sonnet 5 (14.190 + 13.519 tokens, `fase1/diseno_ilu_1.md` y
`diseno_ilu_2.md`; Opus 5-5 no respondió en 2 intentos por el timeout
del cliente, sin medición). Correcciones del integrador contra el
código real marcadas con ◆.

## A. Spot que emite

- `LightData` += `spot_outer_deg: f32` (serde default **45.0**) y
  `spot_inner_deg: f32` (serde default **35.0**); clamp en uso:
  `inner = min(inner, outer)`, `outer ∈ [1°, 179°]`.
- Dirección: forward de la rotación mundial de la entidad,
  convención **−Z** (la misma de las cámaras del motor).
- **UBO de luces: 656 B** (◆ se conserva `shadow[8]` intacto, sin
  trucos de signo ni perder `near`): por luz 5×vec4 —
  `pos_range`, `color_intensity`, `shadow` (como hoy),
  `dir_cos` (xyz = dirección normalizada, w = cos(outer)),
  `cone` (x = cos(inner), y = 1.0 si Spot / 0.0 si Point, zw = 0).
  Total: 5×16×8 + 16 (count+pad) = **656 B**, con su assert.
- Shader (pbr.frag, pbr_legacy.frag; el skinned comparte pbr.frag):
  en el bucle, si `cone.y > 0.5`: `cos_a = dot(-Lp, dir)`;
  `f = clamp((cos_a - cos_outer) / max(cos_inner - cos_outer, 1e-4), 0, 1)`;
  `radiance_p *= f`. El resto del término no cambia.
- Sombra de Spot: reutiliza el cubo puntual tal cual (el cubo es
  omnidireccional; el cono recorta en el shading). Sin cambios en la
  pasada de sombras.

## B. Asignación por objeto con histéresis

- Set 3 pasa a **UNIFORM_BUFFER_DYNAMIC**: un buffer con un
  `PointLightsUbo` (656 B) por objeto; stride =
  `align_up(656, minUniformBufferOffsetAlignment del dispositivo)`;
  capacidad inicial **1024 objetos** (~1 MB máx.). Cada draw enlaza
  el set 3 con el offset dinámico del objeto (los 3 pipelines).
- La app mantiene `HashMap<u64 /*entidad*/, AsignacionLuces>` con:
  lista de ids de luz asignados, posición del objeto al asignar y
  versión del conjunto de luces vista. Reconstruir una lista cuando:
  el objeto se movió > **0,25 m**, cambió el conjunto de luces de la
  escena (versión global), o refresco de seguridad rotativo
  (1/30 de los objetos por frame). ◆ (El diseño de Sonnet proponía
  reconstrucción solo por eventos; la versión global cubre luces que
  se mueven sin que el objeto se mueva.)
- Algoritmo: candidatas = luces (Point+Spot) cuyo rango alcanza a
  la **caja** del objeto: la distancia se mide al **punto más
  cercano de la caja** (centro ± semiejes desde la escala),
  `d = dist(luz, punto_más_cercano) ≤ range`.
  `score = intensidad × prioridad_perfil / (1 + d²/range²)`.
  ◆ (Corrección del 2026-11-13 tras el veredicto visual: la primera
  versión medía al centro menos el radio de la esfera envolvente;
  en un receptor grande — el suelo de 30×18, radio 15 — la d salía
  0 para las 40 velas, el ranking era un empate y el suelo recibía
  4 velas arbitrarias: velas sin charco. **Regla de autoría que
  queda: un objeto recibe como máximo 8 luces; un receptor grande
  se subdivide en piezas** — la escena de validación subdividió
  suelo/techo/paredes en piezas de 6 m y la unión de piezas cubre
  las 40 velas. La salida general, listas por fragmento
  (clustered), queda como bloque futuro Post-1.0.)
  **Histéresis**: las asignadas que siguen en rango se quedan; las
  nuevas solo desplazan a la peor asignada si
  `score_nueva > score_peor × 1,15`; huecos libres se rellenan por
  score desc. Sombras: los 4 cubos siguen siendo GLOBALES (las 4
  luces con cast_shadow elegibles de mayor score medio — en la
  práctica: las 4 primeras por prioridad/distancia a cámara como
  hoy, extendido a Spot); el índice de cubo viaja en `shadow[i].x`
  de cualquier lista donde la luz aparezca.
- Tope por objeto: **por grupo** — para cada grupo presente entre
  las candidatas, tope = mínimo `tope_por_objeto` de los perfiles de
  ese grupo; las listas por grupo se fusionan por score y el total
  se trunca a 8 (los slots del UBO).
- Vistas de portal: reutilizan las mismas asignaciones (el offset es
  del objeto, no de la vista). Sin cambios.
- Fallback: sin asignación calculada (primer frame), lista vacía
  (count=0) — equivalente al comportamiento sin puntuales.
- **Tool MCP nueva** (regla del motor): `glass_light_assignment`
  (entidad → sus luces asignadas con score) para depurar en vivo.

## C. PerfilDeLuces ◆ (componente en la entidad de luz)

◆ El diseño de Sonnet lo ponía como recurso de escena; la escena no
tiene bloque de recursos y el doc del usuario dice «componente».
`EntityData.light_profile: Option<LightProfileData>` (serde
default), componente `"LightProfile"` en la lista de componentes:

```rust
pub struct LightProfileData {
    pub preset: String,          // "Vela" | "Antorcha" | "LinternaArma" | "Panteon" | "Custom"
    pub tope_por_objeto: u8,     // default 4
    pub grupo: u8,               // default 0
    pub prioridad: f32,          // default 1.0
    pub sombra_por_prioridad: bool, // default false
    pub umbral_sombra: f32,      // default 0.0
}
```

Sin componente ⇒ perfil implícito (tope 8, grupo 0, prioridad 1,
sombra según `cast_shadow`). Presets (constructores en código;
rango/intensidad son del `LightData` que el autor pone, el preset
solo fija política):

| Preset | tope | grupo | prioridad | sombra |
|---|---|---|---|---|
| Vela | 4 | 0 | 0,3 | no |
| Antorcha | 4 | 0 | 0,6 | sí (cast_shadow) |
| LinternaArma | 2 | 1 | 1,5 | sí (cast_shadow) |
| Panteón | 4 | 0 | 1,0 | por prioridad (umbral = score medio del frame) ◆ |

◆ «Panteón» con sombra por umbral: con 40 velas ninguna proyecta
salvo las 4 de mayor score si su perfil lo permite — en la práctica
las velas del panteón NO proyectan (tope de cubos) y el preset lo
expresa con `sombra_por_prioridad = true` y umbral alto.

## D. IBL básica de skybox ◆ set 6

- Cubemap procedural en CPU: **64×64×6, R16G16B16A16_SFLOAT** ◆ (el
  renderer es HDR; LDR recortaría el disco solar y mataría el
  especular — se acepta el coste, es 64²), mips 64→32→16→8→4 (5
  niveles; el último ≈ irradiancia). Contenido: gradiente
  hemisférico (los colores de hoy: cielo [0,82, 0,88, 1,0], suelo
  [0,62, 0,58, 0,55], ponderados por la intensidad ambiente viva) +
  disco solar (dirección e intensidad del sol vivo, halo gaussiano).
  Mips por caja (box filter) en CPU. Subida por staging una vez por
  regeneración (◆ nada de pasada de render a cubemap).
- Regeneración: si la dirección del sol cambió > 3° o algún color
  > 0,05 desde la última generación, con throttle de 2 s de juego.
- **Set 6** ◆ (el 1 es el SSBO de huesos del skinned): binding 0 =
  samplerCube; binding 1 = UBO `{ ibl_params: vec4 }` (x =
  intensidad IBL, y = mip máximo, z = 1 si hay cubemap).
- pbr.frag (+ legacy): entorno sustituye al hemisférico SOLO si
  `ibl_params.z > 0.5` (si no, hemisférico de siempre):
  difuso `textureLod(cube, N, mip_max) × albedo × kD × ibl_intensity`;
  especular `textureLod(cube, reflect(-V, N), roughness × mip_max)`
  con Fresnel-Schlick atenuado por rugosidad (aproximación de Karis
  sin LUT: `F0 + (max(1-rough, F0) - F0) × (1-NdotV)^5`).

## Paquetes de implementación

1. **Datos** (scene.rs): campos Spot + `LightProfileData` + presets
   + loader + tests serde. → Sonnet.
2. **Spot emite (global)**: UBO 656 B + shaders + `recopilar`
   incluye Spot con dirección/conos; sesión física: una Spot en el
   campo F10 alumbra el cono y fuera queda oscuro. → Sonnet.
3. **Asignación por objeto**: renderer (set dinámico, buffer,
   offsets por draw) + app (sistema de asignación + histéresis) +
   tool MCP `glass_light_assignment`. → Opus; fallback Sonnet.
4. **C# Light**: `type`, `range`, `spotAngle` + puentes nuevos
   (Init separado, tolerante). → DeepSeek; fallback Sonnet.
5. **IBL**: generación CPU del cubemap + set 6 + términos de entorno
   en shaders + regeneración por umbral. → Sonnet.
6. **Validación**: escena Panteón (40 velas + arma con 6 linternas),
   script de barrido, driver (frametime contra el sistema global —
   medido DESACTIVANDO la asignación por objeto con env
   `GLASS_LUCES_GLOBAL=1` que el paquete 3 dejará como conmutador),
   GIF + capturas → chat principal.

## Riesgos

1. Enlazar set 3 por draw (×3 pipelines + materiales de grafo):
   mitigación — el offset dinámico usa el MISMO set y layout con
   tipo dinámico; verificar que los grafos que usan set 3 compilan
   igual (el layout cambia de tipo: uniform → uniform dynamic, el
   shader no lo distingue).
2. Objetos sin asignación el primer frame a oscuras: mitigación —
   count=0 equivale a «sin puntuales», y la asignación se calcula
   antes del primer render de cada escena.
3. Regeneración IBL en pleno frame (subida de ~200 KB): mitigación —
   throttle 2 s + solo por umbral; la subida va por el staging de
   siempre fuera de la pasada principal.
