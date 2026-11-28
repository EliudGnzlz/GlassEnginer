# INFORME V3 — Texto SDF (cierre de módulo)

Rama `lu/v3-sdf` (base `9a37518`, punta de V2). Commits: V3a `cc2c6e0`,
V3b `f63203b`, V3c `90fb99c`, V3d `d612597`, correcciones de la
revisión `8734916`. Sección propietaria S21; vecinas
S10/S14/S3/S2; S1 sin tocar (V3 no cambia el formato de escena).

## 1. Alcance

V3 sustituye el texto bitmap por texto SDF en cuatro paquetes: V3a
atlas SDF con inserción dinámica; V3b evaluación SDF en el shader y
cableado del atlas (textura apilada); V3c mapa carácter→posición y
desbordamiento; V3d retirada del atlas de bits. El SDF es ahora el
único camino de texto del motor.

## 2. Pipeline local (esta VM)

Build workspace RC 0, build de juego puro RC 0, dotnet RC 0, suite
**1.411/0** (1.408 + 3 tests de las correcciones de la revisión),
clippy RC 0 (solo los 3 avisos de manifiesto preexistentes). Los tres
`.spv` de UI confirmados byte-idénticos a una regeneración fresca con
`glslangValidator -V`.

## 3. CI real de GitHub (rama `lu/v3-sdf`, verificado en la página de Actions)

- V3a `cc2c6e0`: run **#390 Success**, 3m 5s.
- V3b `f63203b`: run **#396 Success**, 2m 57s.
- V3c `90fb99c`: run **#401 Success**, 2m 55s.
- V3d `d612597`: run **#405 Success**, 2m 49s.
- (El run #388 Failure es `c1ac844`, el V3a pre-rebaseo, sin efecto;
  el #376 Failure es `9a37518`, el fallo de CI de V2 ya cerrado por
  `68a31bd`.)

## 4. Prueba física (camino real)

Standalone con la escena fixture (título, botón JUGAR, Score) y con la
escena de desbordamiento: truncar muestra «Este texto es», ellipsis
«Este texto e…» con '…' insertada al vuelo en el atlas; clic medido
en la entidad 12 dispara StartGame. La captura de V3d es
**byte-idéntica a la de V3b** (sha256 `58c6d6359c5a`; texto del botón
565–631, 67 px). Escena de wrap (evidencia de la deuda D-V3-2): el
texto «aaa bbb ccc ddd eee fff ggg hhh» en una caja de 120 px con
`text-overflow: wrap` se dibuja en **una sola línea que desborda la
caja** (captura `captura_wrap.png`, sha256 `47cdd61f…`); el mapa lo
parte en líneas, el pintor no. Es el comportamiento heredado de V2,
documentado, no una regresión de V3.

## 5. Barrido de secciones (S1–S23)

S21 propietaria: pass (físico + suite). S10: pass (dorada + criterios
numéricos G-SDF1/G-SDF3). S14: pass (GameView del editor en V3b con la
UI SDF; la deuda D1 del doble «JUGAR»/«Button» sigue siendo del frente
de huecos de UI del editor, idéntica a V1/V2). S6: pass (`.spv`
verificados byte-idénticos). S1: sin tocar formato (suite). S2/S3: sin
tocar (suite). S22: pass-mínimo (test `prefab_ui` en verde; ningún
prefab con UI en las escenas físicas). S18/S20: en pausa, sin tocar,
no corridas. Resto: suite en verde.

## 6. Consumo total (caracteres de contenido recibidos, medidos por el helper)

- **Opus 119.260**: V3a 90.541 (59.683 de una primera llamada que
  divergió sin entregar código útil + 30.858 de la llamada que
  funcionó); V3b 625; V3c 26.806; V3d 1.288. Freno
  `--max-razonamiento` armado en 25.000 en todas. El gateway no expone
  reasoning de Opus (`reasoning_chars` 0 en todas).
- **Revisión de fin de módulo**: pez 1 Opus 0/4.634; pez 2 DeepSeek
  flash 12.063/2.157.
- **DeepSeek flash (paquetes) útil 2.247** (solo el bloque del parser
  de V3c2; 3 de 4 llamadas se cortaron con parcial, exit 3).
- **Kimi 0** (sin saldo en ningún lado).

## 7. Revisión de fin de módulo (2 peces, solo el documento REVISION_V3.md)

Pez 1 Opus (0/4.634): paráfrasis correcta; 0 Bloqueante, 3 Mayor, 3
Menor. Pez 2 DeepSeek flash (12.063/2.157): 2 Bloqueante, 4 Mayor, 2
Menor. Sin re-auditoría usada.

### Resoluciones

- **CI no confirmado (pez 1 Mayor, pez 2 Bloqueante) → APLICAR**:
  los cuatro commits de V3 están en verde en el CI real (runs #390,
  #396, #401, #405; §3). La rama V3 no tiene el fallo de V2.
- **p99 renegociado (pez 1 Mayor, pez 2 Mayor) → REBATIR con
  evidencia + ANOTAR**: la puerta G-SDF1 es agregada por definición
  (media + p99 sobre todos los píxeles de los 191 glifos, 12/24/48/96
  px); el agregado pasa (media 0,0059, p99 0,1023 ≤ 0,11) con la
  revisión del número aprobada por Rayo según su regla de árbitro.
  Medición por glifo pedida por los peces (a 96 px): '@' 0,1002, '&'
  0,1023, '%' 0,1015, 'Q' 0,0977, 'M' 0,0776, 'W' 0,0288, **'g'
  0,1144, '€' 0,1157** — dos glifos superan 0,11 por separado (el
  mismo error de esquina cóncava, acotado, sin artefacto visible: la
  dorada pasa y el borde mide 0,00 px). ANOTADO: los valores por glifo
  quedan en este informe; si el agregado de una fuente futura supera
  0,11, la puerta falla y el número no se revisa sin orden expresa
  de Rayo.
- **D-V3-2 wrap sin render multilínea (pez 1 Mayor, pez 2
  Bloqueante) → REBATIR (alcance) + ANOTAR con evidencia física**:
  el contrato aprobado de V3c es el mapa (correcto y probado) y el
  pintor emite el texto visible; el render multilínea nunca estuvo
  en el análisis de V3. El pintor sigue emitiendo una sola línea
  (comportamiento heredado de V2, sin regresión). Evidencia física:
  escena de wrap capturada (§4: una sola línea desbordando la caja).
  La deuda D-V3-2 sigue registrada para el frente del pintor.
- **Kerning mapa vs pintor (pez 2 Mayor) → REBATIR en parte +
  ANOTAR (nueva deuda D-V3-3)**: el mapa es correcto (coincide con
  `ab_glyph`); el render nunca aplicó kerning (ni en V2 con el bitmap
  ni ahora). Medido: en «AVAV To Wa» a 16 px la deriva acumulada
  mapa-vs-sin-kern llega a **4,97 px**. D-V3-3: el pintor debe aplicar
  kerning al dibujar (frente del pintor).
- **Capacidad del atlas sin expulsión (pez 2 Mayor, pez 1 Menor)
  → APLICAR (medición) + ANOTAR**: medido con 752 glifos de griego,
  cirílico, armenio, georgiano, latín ext-A, hiragana y muestra CJK:
  **943 glifos en 2 páginas (~470/página), 253 sustituciones** (los
  codepoints sin glifo en DejaVu caen a '?' + aviso, contados). El
  tope de 4 páginas ≈ 1.900 glifos; el repertorio realista con DejaVu
  cabe en 2 páginas. ANOTADO: la política de expulsión queda para un
  frente posterior; CJK necesitaría otra fuente además de expulsión.
- **'\n' en medir (pez 1 Menor, pez 2 Mayor) → APLICAR**: `medir`
  reescrito para medir párrafo a párrafo (igual que el mapa); test
  `medir_con_salto_de_linea_cuenta_dos_lineas` en verde.
- **Truncar con '\n' (pez 1 Menor, pez 2 Menor) → APLICAR**:
  Truncar y PuntosSuspensivos se detienen en la primera línea;
  test `mapa_texto_truncar_y_puntos_se_detienen_en_salto_de_linea`
  en verde.
- **Contorno vacuo sin comprobar (pez 2 Menor) → APLICAR**: test
  `outline_no_existe_como_propiedad` (toda declaración outline se
  rechaza con aviso).

## 8. Deudas

- **D-V3-1**: la herramienta de importación debe rechazar fuentes sin
  contorno con un error visible (frente de herramientas, fuera del
  runtime de V3).
- **D-V3-2**: el mapa parte el texto envuelto en líneas, pero el
  pintor lo emite en una sola línea (heredado de V2); el render
  multilínea queda para el frente del pintor.
- **D-V3-3 (nueva)**: el pintor no aplica kerning al dibujar; la
  deriva medida llega a ~5 px a 16 px en pares con kern fuerte
  (frente del pintor).
- D1 (editor, doble «JUGAR»/«Button») y D2 (standalone 3D negro +
  estela, frente de Rayo) siguen como estaban; D4 (consumo P0/P1 sin
  medir) es deuda de proceso de V2.

## 9. Estado final

Rama `lu/v3-sdf` en `8734916` (V3d `d612597` + correcciones de la
revisión). CI real en verde en los 4 paquetes. Suite local
1.411/0. V3 queda cerrado a falta de la verificación única por
Análisis antes de integrar a `main` (V2 sigue sin integrar por la
misma verificación pendiente).
