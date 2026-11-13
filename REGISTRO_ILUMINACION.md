# REGISTRO — Integración de iluminación completa (2026-11-13)

Bloque ordenado tras la Fase 10 (`INTEGRACION_ILUMINACION.md`). Diseño
consolidado en `DISENO_ILU_FINAL.md` (dos paquetes de diseño de Sonnet 5;
Opus 5-5 murió por timeout del cliente en dos intentos, sin medición).

## P1 — Datos (commit `23409c2`)

- `LightData` += `spot_outer_deg` (def. 45) y `spot_inner_deg` (def. 35)
  con serde defaults y saneado en `spot_cone_cos()`: outer ∈ [1°, 179°],
  inner ≤ outer.
- `LightProfileData` (componente `"LightProfile"` en la entidad de luz):
  `preset`, `tope_por_objeto` (def. 4), `grupo` (def. 0), `prioridad`
  (def. 1,0), `sombra_por_prioridad`, `umbral_sombra`. Presets: Vela
  (tope 4, pri 0,3), Antorcha (4, 0,6), LinternaArma (2, grupo 1, 1,5),
  Panteon (4, 1,0, sombra por prioridad con umbral 1,0). Sin componente ⇒
  perfil implícito (tope 8, grupo 0, prioridad 1,0).

## P2 — La Spot emite (commit `cd09412`)

- UBO de puntuales 400 → **656 B** (por luz: `pos_range`, `color_intensity`,
  `shadow`, `dir_cos` —xyz dirección, w cos_outer—, `cone` —x cos_inner,
  y = 1 si Spot—; `count` en el offset 640).
- pbr.frag y pbr_legacy.frag: factor de cono por cosenos con penumbra entre
  inner y outer. La sombra de la Spot reutiliza el cubo puntual (el cono
  recorta en el shading). Dirección = forward −Z de la matriz de mundo.
- Verificado con `PruebaSpot.scene.json`: charco elíptico cálido con
  penumbra suave bajo el foco.

## P3 — Asignación de luces por objeto (commits `00aa8d4`, `f1afd30`)

- Set 3 pasa a **UNIFORM_BUFFER_DYNAMIC**: buffer de **1024 ranuras**
  (la 0 = global), stride alineado a `minUniformBufferOffsetAlignment`.
- Núcleo puro (`luces_objeto.rs`): candidatas con dist ≤ rango + radio;
  `score = intensidad × prioridad / (1 + d²/rango²)`; **histéresis 1,15**
  (una luz nueva solo desplaza a la peor de su grupo si la supera ×1,15);
  tope por grupo = mínimo `tope_por_objeto` de las candidatas del grupo;
  fusión por score y truncado a 8.
- Sistema: firma FNV-1a del conjunto de luces; re-resolver si el objeto se
  movió > 0,25 m, cambió la firma o toca el refresco rotativo
  (frame+entidad) % 30. Cubos de sombra globales (4 con cast_shadow más
  cercanas al ojo); el índice viaja en `shadow[i].x`.
- Conmutador A/B: `GLASS_LUCES_GLOBAL=1` vuelve al UBO global único.
- Tool MCP nueva (49 en total): `glass_light_assignment` (resumen o por
  entidad: ranura, lista de luces, modo).

## P4 — `Light` desde C# (commit `ead241f`)

- Puente NUEVO `InitLightPropsBridge` (ley del motor: Init separado por
  feature, vinculación tolerante): `light_set_type` (0 Directional,
  1 Point, 2 Spot), `light_set_range`, `light_set_spot_angles`.
- C#: `enum LightType` + propiedades `type`, `range`, `spotAngle`,
  `innerSpotAngle` en `Light` (los setters de ángulo envían siempre el par
  vigente). Managed compila con 0 errores.
- Verificado en vivo: el `Start()` de `LinternaArma.cs` configura las 6
  linternas de la escena de validación por este puente (log
  «Linternas configuradas desde C# por el puente LightProps»).

## P5 — IBL básica de skybox (commit `f7c1a0e`)

- Set 6 (el 1 es el SSBO de huesos): `samplerCube` + UBO `ibl_params`
  (x intensidad, y mip máx, z = 1 si hay entorno).
- Cubemap procedural en CPU: 64×64×6, **R16G16B16A16_SFLOAT** (HDR: el
  renderer es HDR y LDR recortaría el disco solar), 5 mips por box filter.
  Contenido: gradiente hemisférico del motor + disco solar gaussiano con la
  dirección e intensidad vivas del sol. Subida por staging.
- Regeneración si el sol cambió > 3° o algún color > 0,05, con freno de
  2 s. En el shader el entorno sustituye al hemisférico solo si hay
  cubemap: difuso por `textureLod(N, mip_max)` y especular por
  `reflect(−V, N)` con Fresnel-Schlick atenuado por rugosidad (aprox. de
  Karis sin LUT). Sin direccional se conserva el hemisférico de siempre.

## P6 — Validación: escena PruebaPanteon (este commit)

Sala de 30×7×18 m, **111 entidades**: 40 velas con perfil Panteon (18 pares
en filas z=±4,2 + 4 alrededor del altar), 10 columnas, 2 bancos (sondas de
asignación), esfera **espejo** (metálico 1, rugosidad 0,05) sobre el altar,
esfera de **oro**, y un arma con **6 linternas Spot** hijas configuradas
desde C# por el puente de P4 (tipo, rango 26, cono 30°/19°, intensidad 90)
que barre ±26°. Una esfera de paseo cruza la sala por el carril z=2,9.

Resultados (modo por objeto, `glass_light_assignment`):

- **63 objetos con asignación propia** (todas las entidades con malla que
  la necesitan; el resto usa la ranura global 0).
- BancoIzq (oeste): velas 104–107 + linternas 201–202. BancoDer (este):
  velas 128–131 + linternas 205–206. **Conjuntos disjuntos**: cada extremo
  de la sala recibe SUS velas, algo imposible con el tope global de 8.
- La esfera de paseo cambia de conjunto al cruzar (p. ej. de
  {119, 136, 117, 137} a {123, 121, 136, 119}), con solape entre rondas:
  la histéresis evita el parpadeo.
- **A/B de coste** (lavapipe, misma escena): GPU avg por objeto **664 ms**
  vs global **641 ms** (+3,6 % en CPU-render; en hardware el delta es el
  enlace del UBO dinámico por objeto).
- **A/B visual**: en modo global solo iluminan las 8 luces más cercanas a
  la cámara y el fondo de la sala queda plano y muerto; en modo por objeto
  hay charcos cálidos en toda la sala, la cuña de las linternas barre el
  altar y las esferas espejo/oro reflejan las velas de su zona.
- GIF de la validación y capturas A/B en `fase1/test-ilu/` (fuera del repo).

### Incidencias de la validación (honestidad de registro)

1. Las primeras corridas «fallidas» fueron un **error de mi driver**: pasé
   `--scene PruebaPanteon` (nombre) en vez de la ruta
   `assets/scenes/…`; la app cargaba en silencio su escena por defecto.
   De paso destapó un defecto real: **una escena pedida que no carga se
   tragaba el error sin dejar rastro**. Arreglado en este commit: el fallo
   de `load_from_json` ahora se registra con `log::error!` (ruta y causa).
2. Dos poses de cámara de la escena quedaron emparedadas tras columnas de
   la retícula (x=0 y x=5, z=6,5) y devolvían un fotograma uniforme. La
   pose final (noreste, mirando al altar) deja ver sala, arma y paseo.

## Suite y consumo

- Suite completa: **410/410 tests** en verde (395 tras P1–P2, 406 tras P3,
  410 tras P5).
- Consumo del bloque por proveedor: **Claude Sonnet 5: 164.491 tokens**
  útiles (diseño 27.709 · P1 15.029 · P2 15.072 · P3 14.436+15.229+17.659 ·
  P4 15.800+13.877 · P5 12.766+16.914) · **DeepSeek Flash: 29.552** útiles
  (P5 26.016 · scripts P6 3.536) y 25.961 quemados en entregas truncadas o
  vacías · **Kimi: 0** (un 429 «engine overloaded» sin medición) ·
  **Opus 5-5: 0** útiles (dos timeouts del cliente en el diseño, sin
  medición). P6 (diagnóstico, escena y drivers) fue trabajo propio.

## Addenda 2026-11-13 — veredicto visual RECHAZADO y arreglo

**Veredicto de D3m3tr10 sobre las capturas de P6: no aprobado.** En
la captura noreste, las velas cercanas a la cámara eran cilindros
blancos sobre suelo plano, sin charco; la prueba prometía 40 velas
alumbrando la sala. Tenía razón, y el diagnóstico con datos lo
demuestra:

- **La métrica de ranking era el defecto principal.** `resolver`
  medía la distancia al CENTRO del receptor menos el radio de su
  esfera envolvente (radio = 0,5 × escala máxima). El suelo (30×18,
  centro en el medio de la sala, radio 15) tenía a las 40 velas a
  entre 3,0 y 12,7 m del centro → **d = 0 para las 40** → score
  idéntico (intensidad × prioridad, iguales en todas las velas) →
  **empate total**: el conjunto del suelo salió por orden de
  llegada. Consulta en vivo (`glass_light_assignment`): el suelo
  tenía asignadas las velas **[100, 101, 102, 103]** — las cuatro
  del extremo oeste — más 2 linternas; las otras 36 velas no
  aportaban NADA al suelo. Techo y paredes, igual de arbitrarios.
  Los charcos del fondo de la captura eran justo esas 4 velas.
- **Y el límite estructural**: aunque la métrica fuera perfecta,
  un objeto = un conjunto de 8 luces; un suelo de 30×18 jamás
  puede recibir 40 velas en un solo conjunto.

**Arreglo aplicado:**

1. **Motor — distancia a las cotas**: `resolver` recibe los
   semiejes de la caja del receptor y mide la distancia al **punto
   más cercano de la caja** (clamp por eje); candidata si esa
   distancia ≤ rango. 3 tests nuevos que fijan la métrica
   (cotas≠centro, fuera de rango de la caja, y la regresión del
   empate: dos velas sobre la misma caja grande se rankean por su
   distancia real). Diseño actualizado en `DISENO_ILU_FINAL.md`.
2. **Escena — subdivisión de receptores grandes** (la salida real
   dentro de esta arquitectura, y la regla de autoría que queda
   escrita): suelo y techo en piezas de 6×6 m (15+15), paredes en
   segmentos de 6 m (16) — ids 400-445; la escena pasa a 151
   entidades. **Unión de las piezas del suelo: 40/40 velas
   cubiertas**, cada pieza con sus velas locales (la del oeste:
   100-106; la del este: 129-135). Los bancos siguen disjuntos y
   la esfera de paseo sigue cambiando de conjunto.
3. **La salida general** — listas de luces por fragmento
   (clustered/forward+) — queda registrada como bloque futuro
   Post-1.0: es la solución de motor de verdad para receptores
   arbitrarios, con coste de subsistema (clustering + rework de
   shaders), no de parche.

**Re-validación visual** (misma cámara noreste, capturas
`cap_panteon_ne2.png` / `cap_panteon_global2.png` y GIF nuevo en
`fase1/test-ilu/`; el GIF rechazado se conserva como
`panteon_v1_rechazado.gif`): por objeto, la fila cercana a la
cámara alumbra vela a vela en toda su longitud y la sala entera
tiene charcos; en global, la sala queda plana salvo las 8 luces
globales. **A/B de GPU medido de nuevo, espalda contra espalda en
lavapipe: por objeto 1.227 ms frente a global 1.027 ms (+19,5 %
con 103 objetos asignados y la escena subdividida)** — más caro
que el +3,6 % de P6 (63 objetos, escena sin subdividir): el render
por software infla el coste CPU de los enlaces por objeto; se
reporta tal cual se midió.

**Suite tras el arreglo: 429/429.** Consumo del arreglo:
**DeepSeek Flash: 3.796 tokens** (métrica de cotas en
luces_objeto + llamador); diagnóstico, subdivisión, tests y
capturas, trabajo propio. A la espera del nuevo veredicto visual.
