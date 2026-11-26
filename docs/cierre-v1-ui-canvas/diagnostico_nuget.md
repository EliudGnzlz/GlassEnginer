# Diagnóstico nuget.org — fallo nethost-sys en el build del workspace

- Build `cargo build --workspace --locked` (RUSTFLAGS="-D warnings"), rama lu/ui-canvas:
  falló con BUILD_RC 101 en `nethost-sys v0.8.0` (build script): "Failed to query
  nuget.org index for nethost package. Are you connected to the internet?:
  Io(Kind(UnexpectedEof))".
- Todo lo demás compiló bien en ese intento, incluido glass_ui.
- Verificación del vigilante 2026-11-26 07:23: `api.nuget.org/v3/index.json`
  responde 200 en ~1 s por el proxy de salida. Verificación propia en el mismo
  momento: (ver salida del curl que acompaña a esta nota).
- Conclusión: el fallo fue casi seguro transitorio, no un bloqueo del proxy ni
  de la red. Al reintentar la compilación con la aprobación de D3m3tr10, lo
  más probable es que pase a la primera.
- La aprobación de la compilación anterior caducó (pedida 10:50 UTC, venció
  11:00 UTC): no se relanza sin que D3m3tr10 lo pida expresamente.

## Verificación sin red en la caché de Cargo (2026-11-26, diagnóstico Gemini Pro)

- `nethost-sys 0.8.0` (caché local de Cargo): la descarga vive tras la feature
  `download-nuget` y usa `ureq 3.3` con `ureq::agent()` sin configurar proxy:
  no respeta `HTTPS_PROXY`, conecta directo, y la salida directa de esta VM
  se resetea a veces → `UnexpectedEof`. Que curl responda 200 no prueba nada:
  curl sí pasa por el proxy.
- La descarga se cachea en `OUT_DIR/nethost/<target>`: si el directorio existe
  y no está vacío, el build script no vuelve a descargar. Por eso el reintento
  (build_workspace3, RC 0) pasó: la conexión directa funcionó esa vez y la
  caché quedó en `target/` (persistente en home).
- Si el fallo reaparece: predescargar el .nupkg con curl (por el proxy) y
  poblar esa caché, o apuntar al nethost que trae el SDK de .NET 8 en
  `<DOTNET_ROOT>/packs/Microsoft.NETCore.App.Host.linux-x64/<version>/runtimes/linux-x64/native/`.

## Alternativas si el fallo reaparece (orden, diagnóstico Gemini Pro)

1. Apuntar el build al nethost que ya trae el SDK de .NET 8 en disco
   (`<DOTNET_ROOT>/packs/Microsoft.NETCore.App.Host.linux-x64/<version>/runtimes/linux-x64/native/`).
2. Predescargar el .nupkg con `curl -L` desde el flatcontainer de nuget.org
   (`runtime.linux-x64.Microsoft.NETCore.DotNetAppHost`) y alimentar el build
   en local (poblar la caché `OUT_DIR/nethost/linux-x64`).
3. `dotnet restore` de un proyecto mínimo para cachear el paquete en
   `~/.nuget/packages` (el CLI de dotnet sí respeta el proxy).
4. Último recurso: `[patch.crates-io]` con nethost-sys vendorizado cuyo
   build.rs lea archivos locales. Si un arreglo exige tocar el manifiesto del
   workspace más allá del entorno local, se plantea como DUDA antes de
   aplicarlo — no se aplica por cuenta propia.
Estado: no hizo falta ninguna — el reintento pasó (RC 0) y la caché quedó
poblada en disco.
