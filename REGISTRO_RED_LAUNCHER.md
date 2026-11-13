# Red del Launcher — GitHub Releases (2026-11-13)

Orden de D3m3tr10 (2026-11-13 11:16, desde el chat principal): que el
Launcher descargue las actualizaciones del motor directamente de
GitHub Releases.

## Qué se hizo

**Módulo nuevo `crates/glass_editor/src/updates.rs`** (registrado en
`lib.rs`), en dos mitades: lógica pura (versiones, elección de activo,
hash, registro) y red/instalación.

- **Sin tokens, nunca.** El Launcher no guarda credenciales: solo
  puede leer repositorios públicos. Si el repo es privado, la API de
  GitHub responde 404 sin autenticación y la UI lo dice tal cual.
- **Consulta**: `GET {api}/repos/{owner}/{repo}/releases/latest`.
  Compara el tag (sin la «v») con `ENGINE_VERSION` (`0.1.0-alpha`)
  con `semver`; solo avisa si el remoto es estrictamente más nuevo.
- **Hash obligatorio antes de instalar**, por este orden de fuentes:
  campo `digest` del activo en la API → activo `checksums.txt` /
  `SHA256SUMS.txt` del release → activo `<nombre>.sha256`. Sin hash
  publicado o sin coincidencia, **no se instala nada**.
- **Instalación modelo Hub**: el `.zip` se extrae en
  `versions/<versión>/` (con `enclosed_name`, anti zip-slip) y queda
  registrado en `launcher.json` (raíz de datos). Las versiones
  conviven; ninguna se borra y los proyectos no se tocan.
- **Repo configurable** (`owner/repo`, por defecto
  `EliudGnzlz/GlassEnginer`) editable desde la propia pestaña y
  persistido en el registro. Servirá igual cuando el repo sea
  público o exista un repo público de distribución, sin rehacer nada.
- **Overrides por entorno**: `GLASS_UPDATE_API` (def.
  `https://api.github.com`; también sirve para GHES/mirrors y para la
  fixture de prueba) y `GLASS_LAUNCHER_HOME` (def. `~/GlassEnginer`).
- HTTP con `ureq` 3 (rustls); su configuración por defecto lee el
  proxy de las variables de entorno, así que tras un proxy normal
  también funciona. Dependencias nuevas de `glass_editor`: `ureq`,
  `zip`, `semver`, `sha2` (esta última ya era del workspace; las
  demás ya estaban resueltas en el árbol por otras vías, el lock solo
  gana los bordes nuevos).

**UI — pestaña Instalaciones reescrita** (`launcher.rs`): fila de la
copia en uso (versión desde `ENGINE_VERSION`, ya sin literales
repetidos: también el pie del Hub y la vista de crear proyecto),
filas de las versiones descargadas con su ruta, ajuste del
repositorio con Guardar, botón «Buscar actualizaciones» activo que
trabaja en un hilo con canal (la UI no se congela) y, si hay versión
nueva no instalada, botón «Descargar e instalar». Errores honestos y
visibles, incluido el 404 de repo privado. El Launcher sigue siendo
offline salvo esta pestaña, y solo cuando el usuario lo pide.

Tests del módulo (7 nuevos; `glass_editor`: 23 en total):
versiones/novedad, SHA-256 conocido, digest con/sin prefijo, parseo
de checksums, elección de activo por plataforma, extracción de un
zip en memoria y roundtrip del registro JSON.

## Verificación real (VM, 2026-11-13)

Fixture local fiel a la API (`fase1/fixture_gh.py`: sirve el JSON de
`releases/latest` con tag `v9.9.9` y un activo
`glassenginer-v9.9.9-linux-x64.zip` con su `digest` real, y el zip
con un `glass_app` de juguete + `assets/version.txt`), con
`GLASS_UPDATE_API=http://127.0.0.1:8799` y un
`GLASS_LAUNCHER_HOME` temporal, conduciendo el Launcher de verdad:

1. **Detección**: «⬆ Hay una versión nueva: v9.9.9 (esta copia es
   v0.1.0-alpha)» con su botón de descarga. La petición llegó al
   fixture con el repo configurado.
2. **Instalación**: descarga el zip, verifica el SHA-256, extrae en
   `versions/9.9.9/` (`glass_app` y `assets/version.txt` en disco) y
   registra en `launcher.json`; la UI lista la fila «GlassEnginer
   v9.9.9 · Descargada · ✓ Instalada».
3. **Hash adulterado** (fixture con digest `000…`): la descarga
   ocurre pero la instalación se **rechaza** — «⚠ El SHA-256 no
   coincide: la descarga no es íntegra o no es el archivo publicado.
   No se instaló nada.» — y en disco no queda ni la carpeta de datos.
4. **GitHub real**: sin el override, la petición sale pero en esta
   VM el proxy de salida intercepta TLS con una CA privada que
   rustls (raíces Mozilla) no reconoce («invalid peer certificate:
   UnknownIssuer») — artefacto del entorno de la VM, no del código:
   en una conexión normal el certificado de GitHub verifica. Por
   curl, la API sin autenticar confirma el comportamiento esperado
   hoy: **HTTP 404** (repo privado, sin token = no existe).

**Paso exacto pendiente de su decisión de visibilidad**: cuando el
repo sea público (o exista un repo público de distribución y se
ponga en el ajuste), la misma ruta ya implementada — https, API,
descarga con redirección de GitHub, hash e instalación — funciona
sin tocar una línea. La descarga real desde GitHub no se pudo
verificar en esta VM por el 404-by-design del repo privado y por la
intercepción TLS del proxy de la VM; no se finge lo contrario.

Capturas de las tres pasadas en `fase1/test-red/`.

## Consumo

- DeepSeek: **29.378** tokens (18.996 quemados en el primer intento
  del archivo entero — razonamiento sin contenido —; la partición en
  dos paquetes pequeños entregó a la primera: 5.608 + 4.774).
- Claude 0 · Kimi 0 · Opus 0. La UI y la integración son de Rayo.

## Addenda (2026-11-13): primer Release publicado

Por orden suya del mismo día se creó el primer Release del
Launcher, `v0.1.0-alpha`:
https://github.com/EliudGnzlz/GlassEnginer/releases/tag/v0.1.0-alpha

- Activo: `GlassEnginer-Launcher-0.1.0-alpha-linux-x64.zip`
  (build `--release` de `glass_app` + LEEME; SHA-256
  `abb34b70f4c11dfdd811c305c8abdc3a012ca6d077d818befc059c7723ff0ea3`)
  y `checksums.txt` en el formato que `expected_sha256` consume.
- El binario del paquete se probó antes de publicar: abre el Hub
  marcando «v0.1.0-alpha · Vulkan 1.3 · Modo local (sin conexión)».
- El README ganó la sección «Descargar el Launcher» con el enlace.
- Sigue pendiente SU decisión de visibilidad: con el repo privado,
  el enlace funciona con su sesión, pero la comprobación automática
  del Launcher recibe 404 sin autenticar (sin tokens por diseño).

## Addenda 2026-11-13 (tarde) — Instaladores y Release v0.1.1-alpha

- Orden suya (13:49): instaladores del Launcher para probar en
  distintos sistemas. Workflow `.github/workflows/release.yml`
  (commit `8c0a9e4`, CI #50 verde): jobs windows (Inno Setup) y
  linux (zip + AppImage) + publish con `checksums.txt` en el
  formato que el propio Launcher verifica. `ENGINE_VERSION`
  0.1.1-alpha.
- Corrida 1: Linux OK (artefacto 44,9 MB); Windows FALLÓ — la
  primera compilación en Windows de la historia del motor destapó
  5 errores E0425 en `main.rs`: el bloque de señales POSIX
  (SIGHUP/sigaction/__errno_location) era Unix puro. Arreglo
  mínimo (commit `4c8265f`, CI #53 verde): `cfg(unix)` sobre el
  handler y el instalador, no-op `cfg(not(unix))`.
- Corrida 2 (sobre `4c8265f`): LOS TRES JOBS EN VERDE (Windows
  17m 3s, Linux 7m 41s, publish 12s). **Release v0.1.1-alpha
  PUBLICADO**: `GlassEnginer-Launcher-0.1.1-alpha-windows-x64-setup.exe`
  (15,9 MB), `...-linux-x64.zip` (22,6 MB), `...-linux-x64.AppImage`
  (22,8 MB) y `checksums.txt`, cada uno con su SHA-256 en la API.
  El paquete lleva el payload completo (binario + managed/ +
  assets/), no solo el ejecutable como el de v0.1.0-alpha.
- Nota conocida del propio workflow: este release no trae zip de
  Windows, así que el auto-update en Windows no encontrará paquete
  (el .exe es instalación manual); en Linux el zip/AppImage sí
  alimentan la ruta de descarga. Y con el repo privado, la
  descarga automática sigue dando 404 sin autenticar (decisión de
  visibilidad pendiente de él).
- Consumo del bloque: Opus 5.5: 18.551 · resto 0.
