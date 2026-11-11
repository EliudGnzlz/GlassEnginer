# Física de GlassEnginer (Fase 1)

GlassEnginer utiliza Rapier3D 0.23 como base de su simulación física.
Este documento describe el modelo de escena, las consultas y los eventos disponibles para quien desarrolla con el motor.

La Fase 1 añade capas, máscaras de colisión y triggers sin romper la compatibilidad con escenas anteriores.

## Modelo de datos (escena)

Cada entidad con cuerpo físico incluye un bloque `physics`.
Sus campos principales son:

- `body_type`: tipo de cuerpo, `Static` o `Dynamic`.
- `mass`: masa del cuerpo.
- `friction`: fricción del material físico.
- `restitution`: restitución o rebote.
- `box_extents`: dimensiones utilizadas cuando la forma es una caja.
- `linear_velocity`: velocidad lineal del cuerpo.

Desde la Fase 1 están disponibles, además:

- `layer`: capa del collider, de tipo `u8`; valor por defecto `0`.
- `collision_mask`: máscara de interacción, de tipo `u32`; por defecto tiene todos los bits activados.
- `is_trigger`: indica si el collider actúa como sensor; valor por defecto `false`.

Estos tres campos tienen valores por defecto.
Por tanto, una escena antigua que no los declare carga con el mismo comportamiento general:
capa `0`, interacción con todas las capas y respuesta física normal.

### Forma del collider

La forma no se configura como un collider independiente arbitrario: se infiere del modelo asociado a la entidad.

Las formas disponibles son:

- Esfera, con radio igual a la escala dividida entre dos.
- Cápsula.
- Cilindro.
- Caja, utilizando `box_extents`.

También se puede generar un casco convexo por código.

Al preparar una entidad, conviene comprobar que el modelo elegido produce la forma esperada.
`box_extents` solo participa en el caso de la caja; no sustituye a la inferencia de las demás formas.

## Capas y máscaras

`layer` identifica la capa a la que pertenece un collider.
`collision_mask` determina con qué capas puede interactuar.

Rapier aplica una comprobación en ambos sentidos:
dos colliders A y B interactúan únicamente cuando se cumplen las dos condiciones siguientes.

1. El bit de la capa de A está incluido en la máscara de B.
2. El bit de la capa de B está incluido en la máscara de A.

La interacción es simétrica.
Basta con que una de las dos máscaras excluya la capa del otro collider para que no se toquen.

En forma conceptual:

```text
interactúan(A, B) =
    máscara_de_B contiene capa_de_A
    Y
    máscara_de_A contiene capa_de_B
```

Esto permite expresar tanto exclusiones mutuas como relaciones en las que solo una parte necesita configurarse.
No basta con incluir al otro collider en la máscara propia si la máscara del otro ya excluye la capa propia.

### Ejemplo: proyectil y jugador

Un caso habitual es colocar los proyectiles en la capa `4`.
La máscara del proyectil puede excluir el bit correspondiente a la capa del jugador.

Con esa configuración, el proyectil no interactúa con el jugador aunque este último conserve una máscara con todos los bits activos.
La exclusión en uno de los sentidos es suficiente.

Las máscaras también se utilizan al filtrar consultas al mundo.
Una consulta con máscara no equivale a cambiar las máscaras de los colliders de la escena.

## Triggers

Establecer `is_trigger: true` convierte el collider en un sensor.

Un sensor:

- No genera respuesta física.
- No sirve como superficie sobre la que apoyarse.
- No detiene a otros cuerpos mediante una colisión sólida.
- Sí genera eventos de entrada y salida.

Los triggers son adecuados para zonas de meta, áreas de daño y volúmenes de recogida.
La entidad conserva su presencia en el mundo físico para detectar solapes, pero sin la respuesta de contacto de un collider normal.

En los eventos y en los callbacks se conserva la distinción entre colisión y trigger.
De este modo, la lógica de juego puede usar `OnTriggerEnter` para una zona sin tratarla como una colisión sólida.

## Consultas al mundo (desde Rust y desde MCP)

El mundo físico ofrece las siguientes consultas:

| Consulta | Resultado principal |
| --- | --- |
| Rayo | Impacto filtrado por la consulta |
| Barrido de esfera (`SphereCast`) | Entidad, punto, normal y distancia |
| Barrido de cápsula | Entidad, punto, normal y distancia |
| Solape de esfera | Lista de entidades |
| Solape de caja | Lista de entidades |

Todas las consultas aceptan una máscara de capas.
También permiten excluir una entidad, por ejemplo para que el origen de un barrido no seleccione a quien lo realiza.

Los barridos devuelven información geométrica del impacto además de la entidad alcanzada.
Los solapes, en cambio, devuelven la lista de entidades encontradas en el volumen consultado.

Estas operaciones permiten comprobar líneas de visión, proximidad o espacio ocupado sin añadir cuerpos temporales a la escena.

### Pruebas en vivo mediante MCP

La tool `glass_physics_cast` permite ejecutar las consultas contra el motor en ejecución.
Su parámetro `kind` selecciona el tipo:

- `ray`
- `sphere`
- `capsule`
- `overlap_sphere`
- `overlap_box`

La misma superficie de consulta se puede utilizar durante la depuración para contrastar máscaras, exclusiones y resultados sin modificar la escena.

## Eventos de contacto

El mundo emite eventos `Enter` y `Exit` por cada par de entidades.
Cada evento incluye un flag que indica si se trata de un trigger.

El motor mantiene los pares activos.
El evento `Stay` no se emite como un tercer evento independiente del mundo: lo deriva el consumidor a partir de esos pares.

El ciclo conceptual es:

- `Enter`: el par comienza a estar activo.
- `Stay`: el par continúa activo.
- `Exit`: el par deja de estar activo.

### Integración con C#

Los scripts reciben la información mediante `Collision`.
Sus datos relevantes en esta fase son:

- `gameObject`: la otra entidad del contacto.
- `isTrigger`: indica si el contacto corresponde a un trigger.

`GlassBehaviour` expone los callbacks virtuales:

- `OnCollisionEnter`
- `OnCollisionStay`
- `OnCollisionExit`
- `OnTriggerEnter`
- `OnTriggerStay`
- `OnTriggerExit`

Los callbacks de colisión representan contactos con respuesta física.
Los callbacks de trigger representan la actividad de sensores.

Si un callback lanza una excepción, el motor la registra.
La excepción no derriba el motor ni interrumpe su funcionamiento global, pero debe localizarse en el registro para corregir el script.

### Inspección en vivo

La tool MCP `glass_list_contact_events` permite inspeccionar:

- Los pares activos.
- Los eventos recientes.
- El contador de substeps.

Es útil para distinguir entre una consulta que no encuentra nada, un contacto filtrado por máscaras y un par que permanece activo sin volver a producir `Enter`.

## Rendimiento y escenas de estrés

El paquete incluye las escenas `StressTest100`, `StressTest500` y `StressTest1000`.
Se generan mediante `scripts/gen_escenas_estres.py`.

Cada escena contiene una rejilla de esferas y cajas dinámicas sobre un suelo.
Su propósito es medir cómo crece el coste del paso físico al aumentar el número de cuerpos.

El coste del frame puede observarse con `glass_frame_timing`.
Para comparar resultados, conviene mantener la misma escena y las mismas condiciones de ejecución entre mediciones.

Estas escenas son pruebas de carga, no referencias de comportamiento de juego.
Sirven tanto para detectar regresiones como para comprobar el efecto de cambios en la simulación o en el filtrado.

### Calibración del movimiento dinámico

El motor hereda dos decisiones de diseño que afectan a todas las escenas de estrés y a los cuerpos dinámicos:

- Las rotaciones están bloqueadas.
- La amortiguación lineal es `1.5`.

En consecuencia, los cuerpos caen frenados y alcanzan una velocidad terminal aproximada de `6,5 m/s`.
Este valor debe tenerse en cuenta al calibrar movimiento, tiempos de caída y expectativas de las pruebas; no indica por sí solo un error de integración.

## Límites conocidos

El modelo de datos de escena admite un solo collider por entidad.
El mundo físico sí admite varios colliders por entidad cuando se construye por código, pero los colliders compuestos configurables en escena quedan como trabajo futuro.

Todavía no hay consultas físicas disponibles desde C#.
La superficie equivalente a `UnityEngine.Physics` llegará en una fase posterior.

En la Fase 1, los rayos, barridos y solapes se realizan desde Rust o mediante MCP.
Los scripts C# reciben eventos de contacto, pero no deben asumir que ya existe una API C# para consultar el mundo.

## Caja de herramientas de colliders (Fase 1.5)

### Colliders primitivos compuestos

El campo `colliders` del physics de una entidad permite combinar varios colliders primitivos en un mismo cuerpo. Cada elemento admite:

- `shape`: `"Box"`, `"Sphere"`, `"Capsule"` o `"Cylinder"`.
- `size`: dimensiones o parámetros de la forma, según el tipo de primitiva.
- `offset`: desplazamiento del collider en el espacio local de la entidad.
- `is_trigger`: marca ese collider concreto como trigger, independientemente de los demás.

La capa y la máscara de colisión se configuran en la entidad mediante `layer` y `collision_mask`, y se aplican al conjunto. Si `colliders` es una lista vacía, se mantiene el comportamiento histórico: el collider se infiere automáticamente a partir de la entidad.

### Colliders de malla

El campo `mesh_collider` selecciona cómo se usa la geometría del modelo:

- `"None"`: no se crea un collider de malla; se usan los primitivos compuestos o el collider inferido.
- `"Convex"`: construye el casco convexo de la geometría retenida del modelo.
- `"TriMesh"`: usa la malla exacta, incluidos sus huecos y concavidades. Solo es válido para cuerpos `Static`. En cuerpos dinámicos o kinemáticos se degrada automáticamente a `"Convex"` y se escribe un aviso en el log.

Si el modelo no tiene geometría retenida —por ejemplo, porque es un modelo integrado o no se ha importado— no se puede construir el collider de malla. En ese caso se vuelve a los colliders primitivos y se registra un aviso.

### Geometría retenida

Al importar un modelo, la aplicación conserva sus posiciones e índices en CPU dentro del `AssetCache`. Esta copia es la que permite construir cascos convexos y TriMeshes sin depender de lecturas desde GPU. La geometría CPU se expulsa junto con la malla GPU correspondiente, por lo que no sobrevive a la descarga del asset.

### Límite conocido

La carga de escena resuelve tanto colliders primitivos compuestos como colliders de malla. El spawn en vivo mediante `Instantiate`, sin embargo, actualmente solo resuelve primitivos compuestos: no crea colliders `"Convex"` ni `"TriMesh"` a partir de la malla.
