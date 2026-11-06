# GlassEnginer

Motor de videojuegos open source tipo Unity, en desarrollo activo (beta). Núcleo en Rust con render Vulkan 1.3, scripting en C# con API estilo Unity y un servidor MCP nativo pensado como vía principal para trabajar el motor con IA.

Versión actual: **0.1.0**.

## Qué es

Lo que está implementado hoy, verificado en el código:

- **Editor visual** con Launcher inicial estilo Hub antiguo de Unity: tema claro, proyectos en cuadrícula y siempre offline.
- **Render PBR** con importación glTF de materiales y texturas, mip-maps y materiales por primitiva.
- **Física** con Rapier3D y **audio** con Kira.
- **Scripting C# embebido** (.NET 8) con API estilo Unity, incluido input de teclado y ratón.
- **UI de juego tipo Canvas de Unity** (Canvas, RectTransform, texto, botones) que funciona en el editor y en el juego standalone; los botones reciben clics en el standalone.
- **Servidor MCP nativo** en dos modos: sobre la escena viva del editor y headless con escena propia.
- **Grafo de shaders visual propio** (nodos básicos) con 6 grafos prefabricados editables: agua, brisa, espuma, fragmentos, láser y tormenta de arena. Compilan a SPIR-V.
- **Recarga en caliente de assets** y **puente con Blender**: al guardar un `.blend` o `.fbx`, Blender headless lo convierte a `.glb` y el motor lo refleja.
- **Paquete standalone**: el editor genera `build/standalone/` con el ejecutable del juego, los assets, `project.json` y las DLL de C#.
- **Escenas de ejemplo** en `assets/scenes/`: `SampleScene`, `Nivel2` y `CanvasUiTest`.
- **124 tests automatizados** en verde en la máquina de desarrollo (`cargo test --workspace`); la validación en CI queda pendiente de la primera corrida, cuando el repo se suba.

## Requisitos

- **Rust 1.80 o superior** (edition 2021).
- **Driver Vulkan 1.3**. En Linux sin GPU sirve el render por software (lavapipe: paquete `mesa-vulkan-drivers` y `libxkbcommon-x11-0`).
- **Runtime de .NET 8** para ejecutar los scripts C# de los juegos.
- **SDK de .NET 8**, solo para compilar el proyecto C# (`managed/GlassEngine.Core`). El repo fija `8.0.132` en `global.json`.
- **glslangValidator**, solo para recompilar shaders.
- **Blender** instalado, solo para el puente con Blender.

## Compilar y ejecutar

Compilar todo (con el editor):

```bash
cargo build
```

Añade `--release` para la build optimizada.

### Editor y Launcher

```bash
cargo run --release -p glass_app
```

O, con el binario ya compilado:

```bash
./glass_engine.sh
```

Ambos abren el Launcher y el editor.

### Servidor MCP

**Headless** (sin ventana, JSON-RPC por stdio):

```bash
./target/release/glass_app --mcp
```

También vale `--mcp-stdio`. Requiere la build con editor (la de por defecto).

**Sobre la escena viva**: al lanzar el editor con stdin conectado por pipe, el servidor MCP sirve la escena del editor. Se puede forzar con `--mcp-live`:

```bash
./target/release/glass_app --mcp-live
```

### Juego standalone

Juego standalone puro (sin código de editor):

```bash
cargo build -p glass_app --no-default-features
./target/debug/glass_app --standalone --scene assets/scenes/SampleScene.scene.json
```

### Tests

```bash
cargo test --workspace
```

### Parte C# (opcional)

El motor funciona sin recompilar la parte gestionada, pero si modificas el
proyecto C# necesitas el SDK de .NET 8:

```bash
dotnet build managed/GlassEngine.Core/GlassEngine.Core.csproj
```

## Recompilar shaders

Ejemplo con un shader del repo:

```bash
glslangValidator -V assets/shaders/pbr.frag -o assets/shaders/pbr.frag.spv
```

Los `.spv` de `assets/shaders/` se incrustan en el binario al compilar, así que hay que recompilar el motor después de regenerarlos.

## Estructura del proyecto

- `crates/` — workspace Rust de 8 crates:
  - `glass_core` — escenas y datos.
  - `glass_renderer` — Vulkan con `ash` y `gpu-allocator`.
  - `glass_editor` — editor con `egui`.
  - `glass_scripting` — CoreCLR/.NET 8 embebido vía `netcorehost`.
  - `glass_mcp` — servidor MCP JSON-RPC sobre stdio.
  - `glass_audio` — Kira.
  - `glass_physics` — Rapier3D.
  - `glass_app` — el binario `glass_app`, que reúne editor + launcher o juego.
- `managed/` — proyecto C# `GlassEngine.Core`.
- `assets/` — `scenes`, `shaders`, `shader_graphs`, `models`, `textures`, `fonts`, `prefabs`.
- `tools/` — utilidades Python del proyecto.
- `project.json` — proyecto de ejemplo «GlassPlatformerBeta», escena por defecto `SampleScene`, ventana 1280×720.

## Estado del proyecto

GlassEnginer es una **beta en desarrollo activo**, no una versión estable. Lo que todavía no está:

- Sin sombras en cascada (CSM) ni cadena de post-proceso.
- Sin skinning/animación esquelética de modelos.
- El editor no tiene undo/redo.
- La red del Launcher (descargar el motor, actualizaciones, comunidad) está deshabilitada hasta que exista el repositorio oficial; el Launcher es offline.
- El juego en red (cooperativo y servidor) es un tema de diseño post-beta: no está implementado.

## Licencia

Licencia dual **MIT OR Apache-2.0**. Consulta el fichero `LICENSE` en la raíz del repositorio.