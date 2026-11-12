# Bloque ONLINE — bloque formal de la cola (registrado el 2026-11-12, precisión del mismo día)

**Estado: EN COLA, sin implementar.** Va justo después de la prueba integral
de sistemas. No es una nota post-beta: es un bloque con nombre propio,
elevado por decisión expresa de D3m3tr10 el 2026-11-12.

## Por qué existe este bloque

La auditoría Opus del 2026-11-12 no encontró código online porque **no hay
código online que auditar**: lo único previo es la nota de diseño post-beta
del 2026-11-05 (`DISENO_ONLINE_POSTBETA.md`, en `~/workspace/glass-engine/`).
El online es importante para el proyecto y no puede quedar como nota suelta:
este documento lo convierte en bloque formal con criterio de cierre.

## Alcance del bloque: P2P con anfitrión (y solo eso)

**Lo primero y más importante del online es el sistema P2P con anfitrión**:
un jugador aloja toda la partida (listen server, con autoridad del host en
simulación, física y combate) y los demás se unen a ella. Objetivo de
escala: **4 a 8 jugadores**.

**El servidor dedicado autoritativo de 50+ SALE de este bloque**: pasa a la
lista de después como escalón posterior independiente, fuera del bloque
online.

## Exigencia central: replicación sin desync perceptible

La replicación debe actualizar bien todo — transforms, estado de entidades
y eventos de gameplay — con el **desync/lag perceptible mínimo posible**,
sabiendo que el suelo lo marca la latencia del anfitrión. El diseño cerrado
ataca eso de frente:

- **Interpolación** de entidades remotas.
- **Predicción** del jugador local donde aplique.
- **Tasa de snapshots y compresión delta ajustables**.
- **Correcciones suaves**: nada de teletransportes visibles en condiciones
  normales.
- Declaración honesta en el diseño de **qué se puede enmascarar y qué no**:
  la promesa del motor es la herramienta optimizada, no magia.

## Escalones del bloque

### (a) Diseño cerrado de la capa de red — `glass_net`

Diseño completo a partir de `DISENO_ONLINE_POSTBETA.md`: transporte,
descubrimiento/unión a la partida del anfitrión, autoridad del host,
modelo de replicación (snapshots, delta, interpolación, predicción,
corrección suave), tick y gestión de desconexiones, y superficie para C#
y MCP. **Se presenta a D3m3tr10 para su aprobación antes de escribir
código.**

### (b) Implementación del P2P con anfitrión (4–8 jugadores)

Listen server jugable con la replicación del diseño. Cierre con la
verificación de abajo superada.

## Verificación del bloque (al construirlo)

**Latencia simulada** al host de **50 / 100 / 150 ms**, midiendo:

- Error de posición **medio y máximo** de las entidades replicadas.
- Número de **correcciones bruscas** (teletransportes perceptibles).
- **Ancho de banda por cliente**.

Sin esas tres métricas en los tres escalones de latencia, el bloque no se
cierra.

## Orden en la cola

1. Pasada de medios de la auditoría — **cerrada** (2026-11-12).
2. Fase 9, escena escaparate — **cerrada** (2026-11-12).
3. Prueba integral de sistemas — en curso al registrar este bloque.
4. **Bloque ONLINE (este documento)**: (a) diseño cerrado del P2P →
   aprobación, (b) implementación P2P con anfitrión 4–8.

Las fases 10 (demo FPS) y 11 (medición del generador) siguen aparcadas.
El servidor dedicado de 50+ y el port del player a Android 10+ quedan en
la **lista de después** (`LISTA_DE_DESPUES.md`), como ítems posteriores
independientes de este bloque.
