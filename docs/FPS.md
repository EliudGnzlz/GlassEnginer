# Sistema FPS — Fase 10

## 1. Alcance

GlassEnginer incluye un sistema FPS con:

- Armas configuradas por datos, en modo proyectil o hitscan.
- Movimiento WASD y control de yaw/pitch desde la entidad cámara.
- Mira con zoom mediante el FOV real de la cámara.
- Proyectiles físicos con gravedad, amortiguamiento y pool reutilizable.
- Impactos, agrupación y caída medida en dianas.
- Explosiones del RPG y barriles explosivos encadenables.
- Escenas de prueba balística y de estrés físico.

Scripts principales:

| Script | Responsabilidad |
|---|---|
| `SistemaArmas` | Carga `assets/data/armas.json` y proporciona las definiciones de armas. |
| `JugadorFps` | Movimiento WASD, yaw/pitch y mira sobre la entidad cámara. |
| `ProyectilF10` | Pool de proyectiles; cada proyectil crea su propio cuerpo físico. |
| `ProyectilRpg` | Impacto, explosión, impulso, daño y efectos del RPG. |
| `Diana` | Registro de impactos, agrupación y caída medida. |
| `BarrilExplosivo` | Detonación por daño acumulado y propagación a barriles vecinos. |
| `BlancoMovil` | Diana desplazada sobre un raíl. |

## 2. Armas por datos

Las armas se definen en `assets/data/armas.json`. `SistemaArmas` lee el archivo al arrancar.

| Campo | Unidad o valores | Uso |
|---|---|---|
| `nombre` | Texto | Identificación del arma. |
| `modo` | `"proyectil"` o `"hitscan"` | Modelo de disparo. |
| `velocidad` | m/s | Velocidad inicial; solo proyectiles. |
| `dispersion` | Grados | Dispersión del disparo. |
| `alcance` | m | Alcance configurado. |
| `dano` | Valor de daño | Daño aplicado por el arma. |
| `zoom_fov` | Grados de FOV | FOV de cámara al apuntar. |
| `cargador` | Número de disparos | Capacidad del cargador. |
| `perdigones` | Número de proyectiles | Proyectiles por disparo; la escopeta usa 8. |
| `radio_explosion` | m | Radio de explosión; solo RPG, con valor 9,0. |

Armas incluidas:

| Arma | Modo | Datos destacados |
|---|---|---|
| Pistola 9mm | Proyectil | 120 m/s. |
| AR 5.56 | Hitscan | Sin proyectil físico. |
| Escopeta 12 | Proyectil | 90 m/s, 8 perdigones y 3,5° de dispersión. |
| Francotirador .308 | Proyectil | 260 m/s. |
| RPG-7 | Proyectil | 55 m/s y radio de explosión de 9 m. |

### Añadir un arma

1. Añade su entrada en `assets/data/armas.json`, con los campos aplicables a su modo.
2. Añade la misma definición a la tabla espejo de `SistemaArmas`.
3. Comprueba que nombre, modo y valores numéricos coinciden en ambos sitios.
4. Reinicia la ejecución: el JSON se carga al arrancar.
5. Prueba disparo, alcance, dispersión, cargador y mira. Para un RPG, prueba también la explosión.

Si el JSON falla, el sistema usa la tabla espejo en código. El JSON no es, por tanto, la única fuente que debe actualizarse: una definición desincronizada puede producir comportamientos distintos según se use el archivo o el respaldo.

## 3. Modos de disparo

### Proyectil

Crea un proyectil del pool de `ProyectilF10`, con un cuerpo dinámico sometido a gravedad y amortiguamiento lineal. La trayectoria y la caída son físicas, no una corrección visual.

Ejemplo medido: el francotirador a 100 m presenta una caída de **1,46 m**.

Usa este modo cuando necesites trayectoria visible, tiempo de vuelo, caída balística, perdigones físicos o proyectiles explosivos.

**Tunelización:** un proyectil rápido puede atravesar blancos finos sin registrar el impacto. A 260 m/s avanza aproximadamente **4,3 m por paso físico**. Para blancos lejanos y finos, usa hitscan o discos físicamente gruesos; no dependas de proyectiles rápidos contra geometría delgada.

### Hitscan

Resuelve el disparo sin proyectil físico. Es apropiado para armas de respuesta inmediata y para evitar los problemas de tunelización de proyectiles rápidos.

No proporciona tiempo de vuelo ni caída física de un proyectil.

## 4. Mira y FOV

`JugadorFps` controla WASD y la orientación yaw/pitch desde la entidad cámara.

Al apuntar, la mira cambia el FOV real mediante `RuntimeBridge.SetCameraFov`:

- FOV normal: **62°**.
- FOV con mira: el valor `zoom_fov` del arma equipada.

El zoom no es únicamente un efecto de interfaz: modifica la proyección de la cámara. Cada arma puede definir una ampliación distinta mediante `zoom_fov`.

## 5. Explosiones

### RPG-7

Al impactar, `ProyectilRpg`:

1. Detecta los cuerpos dentro del radio mediante `OverlapSphere`.
2. Aplica un impulso radial con caída según la distancia.
3. Registra el daño mediante el registro de impactos.
4. Reutiliza y teletransporta fogonazo, onda y humo al punto de explosión.
5. Activa una luz flash.

El radio se configura con `radio_explosion`; el RPG incluido usa **9,0 m**.

### Barril explosivo

`BarrilExplosivo` acumula daño y detona al alcanzar su condición de explosión. La detonación puede alcanzar barriles vecinos y encadenar nuevas explosiones.

Esto permite diseñar grupos de barriles como propagación física: la distancia entre ellos determina si la cadena continúa.

## 6. Escenas y marcas de log

### `Fase10CampoTiro`

Ejecuta una sesión programada mediante `SesionFps`. No requiere reproducir manualmente cada disparo para obtener la prueba básica.

Busca las marcas `[F10]`:

| Marca asociada | Qué comprobar |
|---|---|
| Disparos | Qué armas se ejecutan durante la sesión. |
| Impactos por diana | Impactos registrados y agrupación. |
| Caída medida | Diferencia vertical observada en la diana. |
| Detonaciones | Activación de explosiones. |
| Fin de sesión | La secuencia terminó; ausencia de esta marca indica una sesión incompleta. |

Contrasta los impactos y la caída del log con la configuración del arma, especialmente al cambiar velocidad, alcance o dispersión.

### `Fase10Cubos<N>`

Variantes disponibles: `Fase10Cubos1000`, `Fase10Cubos5000` y `Fase10Cubos10000`.

Son pruebas de estrés del RPG contra cúmulos de cubos. Las marcas `[CUBOS]` describen la prueba de cúmulos y `[RPG]` los eventos del lanzamiento y la explosión.

Usa las tres escalas para comparar el coste y la estabilidad física al aumentar el número de cuerpos. Un resultado correcto en 1000 cubos no garantiza el mismo comportamiento en 5000 o 10000.