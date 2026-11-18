# Hub y motor: los dos modos de arranque

GlassEnginer es un solo binario (`glass_app`) con dos modos de trabajo.
Esta nota documenta el contrato entre ambos tras la separación Hub/motor
(sub-bloques S0–S10 del diseño de separación).

## Modo separado (el de los usuarios)

- **`glass_app --hub`** arranca el **Hub**: una cáscara ligera con su
  propia versión (`HUB_VERSION`) que no abre ningún proyecto en su
  proceso: sin escena demo, sin runtime de scripts (CoreCLR), sin MCP en
  vivo y sin vigilancia de assets. Su trabajo es gestionar: proyectos,
  ejemplos, instalaciones y complementos.
- Los motores se instalan en `engines/<versión>/` dentro de la raíz de
  datos (`%USERPROFILE%\GlassEnginer` en Windows; `GLASS_LAUNCHER_HOME`
  la sustituye en pruebas). El puntero de cuál está en uso vive en
  `engines.json` y **solo lo escribe el Hub**.
- Al abrir un proyecto, un ejemplo o una escena suelta, el Hub lanza el
  motor en uso como **proceso hijo** con `--project <ruta absoluta>`,
  `--sample <clave>` o `--scene <ruta>`. El hijo abre el editor completo
  y, al cerrarlo («Volver al Hub» o cerrar la ventana), el proceso
  termina: no hay vuelta en caliente al Hub.
- El hijo no hereda el entorno del Hub: se le retiran las variables
  `GLASS_*` de prueba (salvo `GLASS_LAUNCHER_HOME`, que comparte la raíz
  de datos) y `APPIMAGE` (apunta al ejecutable del Hub, no al suyo).
- Todo proceso cuyo ejecutable vive en `engines/<versión>/` toma un lock
  de sesión compartido sobre `engines/<versión>/.sesion.lock`. El Hub lo
  respeta: actualizar el motor con un editor abierto **difiere** el
  borrado de la versión anterior y lo avisa en Instalaciones; el borrado
  se completa al arrancar el Hub sin sesiones vivas. Al arrancar, el Hub
  también reconcilia carpetas de `engines/` que `engines.json` no conoce
  (restos de una instalación interrumpida): pasan a borrados pendientes
  por el mismo camino. Riesgo aceptado: si el Hub muere justo entre el
  renombrado final de una instalación y el guardado de `engines.json`,
  esa instalación —aunque esté completa— se trata como huérfana y se
  borra; se recupera reinstalando la versión. La versión en uso y la
  conservada están protegidas siempre: nunca se borran ni se marcan
  pendientes.
- Actualizaciones: el motor se ofrece en releases `v*` y el Hub en
  releases `hub-v*`. **Nada se actualiza solo**: comprobar, descargar y
  aplicar exigen siempre el clic del usuario. El Hub se reemplaza a sí
  mismo dejando el ejecutable anterior en `<ejecutable>.viejo` (se borra
  al arrancar el nuevo) y se relanza con `--tras-actualizar-hub`, que
  muestra la pantalla de carga de bienvenida.

### Contrato de argumentos (modo separado)

| Argumento | Significado | Incompatible con |
|---|---|---|
| `--hub` | Arranca el Hub | `--project`, `--scene`, `--sample` |
| `--project <carpeta>` | Abre ese proyecto en el editor (modo hijo) | `--hub`, `--scene` |
| `--scene <archivo>` | Abre una escena suelta | `--hub`, `--project` |
| `--sample <clave>` | Abre un ejemplo (`plataformas`, `fisica`, `luces`, `juego`) | `--hub` |
| `--tras-actualizar-hub` | Splash tras actualizar el Hub (lo añade el propio Hub al relanzarse) | — |

Los tres argumentos con valor lo exigen: si el flag queda el último, el
arranque falla con un error que lo nombra. La validación vive en
`validar_argumentos` (main.rs) y es previa a cualquier ventana.

## Modo combinado (el de desarrollo)

Sin `--hub`, el mismo binario hace de Hub y de motor en un solo proceso:
el Launcher abre los proyectos dentro del propio proceso (sin lanzar
hijos) y «Volver al Hub» es un cambio de pantalla, no una salida. Es el
modo cómodo para desarrollar el motor (un solo proceso que depurar,
arranque directo desde el IDE o desde `cargo run`) y el que usan los
tests físicos internos.

Diferencias prácticas frente al modo separado:

- No hay `engines.json` que valga: el «motor en uso» es el propio
  binario, y su versión es `ENGINE_VERSION` en tiempo de compilación.
- No hay lock de sesión (el lock solo se toma cuando el ejecutable vive
  en `engines/<versión>/`), ni borrados diferidos, ni reconciliación.
- Las comprobaciones de actualización se hacen contra `ENGINE_VERSION`.
- La compatibilidad de complementos se juzga contra el propio binario.

El modo combinado es una herramienta de desarrollo: las releases se
publican y se prueban siempre en modo separado.
