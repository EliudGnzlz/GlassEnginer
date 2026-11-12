# Integración de iluminación: Spot que emite, luces por objeto, perfiles e IBL básica

**Orden de D3m3tr10 (2026-11-12):** tras terminar la Fase 10, se
INTEGRA la iluminación acordada — ya no es solo una prueba. Se integra
lo clasificado como NO posterior en el análisis de iluminación de
hoy; el resto del análisis queda Post-1.0 y no se toca.

**Sitio en la cola: al cerrar la Fase 10, antes del bloque online
P2P.** Cierre del bloque: pruebas físicas en la escena de validación
y el GIF en acción para el veredicto de D3m3tr10 en el chat principal.

## Lo que se integra

### 1. La Spot que emite

El tipo Spot **ya existe** — está en `LightData.light_type`
(`"Directional"/"Point"/"Spot"`, scene.rs:402) y el editor lo crea
(inspector y menú) —, pero el renderer lo reconoce y **no lo emite**:
queda fuera del UBO (comentarios en main.rs:6961 y renderer.rs:497:
«en esta v1 no emite»). El trabajo:

- (a) que la Spot emita: cono en el shader y su hueco en la
  selección de luces del frame;
- (b) añadir a `LightData` los campos que le faltan: `spot_angle`
  y el ángulo interior (penumbra);
- (c) exponer en el `Light` de C# lo que hoy no tiene: `type`,
  `range` y `spotAngle` (hoy solo expone `color` e `intensity`,
  UnityEngine.cs:874).

### 2. El complemento por objeto

- **Asignación de luces por objeto**: cada objeto resuelve las luces
  que lo afectan (rango → prioridad → distancia) **con histéresis**,
  para que la lista no baile al moverse; tope por objeto
  configurable. **Sin luz representativa**: cada luz del conjunto
  asignado alumbra de verdad, con su cono y su alcance.
- **Componente `PerfilDeLuces`** con presets:
  - **Vela**
  - **Antorcha**
  - **Linterna de arma**
  - **Panteón**
  Cada preset define: tope de luces por objeto, grupo y si proyecta
  sombras según prioridad.

### 3. IBL básica de skybox

Propuesta de Rayo aprobada por D3m3tr10 al ordenar «integra todo
esto»: cubemap del cielo muestreado por rugosidad como reflejo de
entorno por defecto, para que los metales PBR no se vean muertos en
interiores. **Sin sondas todavía** (las sondas son Post-1.0).

## Pruebas físicas y entregable

- **Escena de validación**: un panteón con **~40 velas** (preset
  Panteón) y un arma con **6 linternas** (spotlights) barriendo una
  pared; cámara en movimiento cruzando los bordes de cupo para cazar
  parpadeos de la asignación; medición de frametime contra la misma
  escena con el sistema global actual.
- **Entregable obligatorio: un GIF de la prueba en acción** (la
  cámara recorriendo el panteón con las velas titilando y el barrido
  de las 6 linternas) + capturas, presentados en el **chat
  principal** para el veredicto visual de D3m3tr10. Sin GIF no se da
  por entregado.

## Post-1.0 (no se toca en este bloque)

Del mismo análisis de iluminación, quedan Post-1.0:

- Area Light (hoy no existe en ninguna forma).
- Cookies de luz.
- Modo Mixed por luz y Shadowmask (hoy hay realtime y bake v1
  global, sin modo por luz).
- Reflection Probes y Light Probes (la IBL de este bloque es solo el
  cubemap del cielo, sin sondas).
- SSR, SSAO, CSM multicapa y volumétricos.
