# GlassEnginer

Motor de videojuegos open source tipo Unity, en desarrollo activo (beta). Núcleo en Rust con render Vulkan 1.3, scripting en C# con API estilo Unity y un servidor MCP nativo pensado como vía principal para trabajar el motor con IA.

Está pensado para flujos procedurales: generar y borrar grandes cantidades de objetos en tiempo real, creando y regenerando mundos repetidamente sin grandes cargas de memoria ni de procesos. Medido con la escalera de spawn/despawn de esferas (informe completo en `INFORME_ESFERAS.md`): **1.000.000 de entidades generadas en ~134 s y borradas en ~141 s, con un pico de ~2,5 GB de RAM y sin fugas entre ciclos**; 100.000 entidades con física dinámica sostenidas en la misma prueba.

Versión actual: **0.1.0**.

## Qué es

Sistemas implementados hoy, verificados en el código:

### Render

- Render PBR con importación de glTF.
- Culling por frustum.
- Texturas BC con compresión en GPU.
- Sombras puntuales en atlas —4 cubos— y sombras de sol con PCF.
- Niebla.
- Post-procesado HDR con ACES, bloom y viñeta.
- Ciclo día/noche en tiempo real.
- Bake de lightmaps v1.
- Portales en vivo, con cruce e impulso conservado.

### Física

- Capas, máscaras y triggers.
- Eventos de contacto expuestos a C#.
- Colliders compuestos y de malla.
- Consultas de raycast, sphere, capsule y overlap.

### Audio

- Buses Master, Music y SFX.
- Ducking.
- Oclusión por rayo.

### Animación

- Animator con FSM y blend tree 1D.
- Skinning por GPU.

### UI

- Canvas con anclas.
- Canvas Scaler.
- Grupos de layout.

### Partículas y núcleo

- Partículas en CPU con pool SoA.
- TransformStore SoA.
- Generación y borrado de grandes cantidades de objetos en tiempo real para crear y regenerar mundos repetidamente sin grandes cargas de memoria ni de procesos.
- Instrumentación de frame y GPU.
- Scripting C# embebido con .NET 8 y API estilo Unity.

### Shaders

- Grafo de shaders rehecho, con propiedades y UBO por instancia.
- Pases Shadow y Outline.
- Presets toon.
- Fallback magenta.

### MCP e IA

- MCP nativo con 48 tools.
- Cola de comandos determinista.

### Assets y editor

- Editor visual con Launcher estilo Hub antiguo de Unity, siempre offline.
- Puente con Blender, con recarga en caliente y LOD automático por sufijo.
- Panel Project completo: carpetas, creación, renombrado y movimiento de archivos, papelera y prefabs desde C#.
- Generación de paquete standalone.

## Demos y escenas de prueba

- **Escena escaparate**: demo del motor — avenida PBR y toon, clima (lluvia y tormenta de arena) y portal en vivo.
- **Demo FPS**: demo del motor que ejercita la física, las consultas, las partículas y la prueba integral en una misma escena.
- **Cúmulos de cubos**: escenas de estrés de spawn y física (1.000 / 5.000 / 10.000 cuerpos).
- **Escalera de esferas**: estrés procedural de spawn/despawn (1.000 / 10.000 / 100.000 / 1.000.000 de entidades, con y sin física).

El acabado visual de las demos está en pulido; su función hoy es ejercitar y medir los sistemas del motor.

## En fase de pruebas

- Cierre de la **Fase 10**: prueba integral del motor.
- Sesión del editor: importación de assets arrastrando desde el sistema operativo.
- Integración de iluminación: Spot emisiva, asignación de luces por objeto con perfiles e IBL básica de skybox.

## Inspiración y dirección de arte

GlassEnginer está inspirado en la forma de trabajar de Unity — su API en C#, su editor por paneles y su flujo de escenas y componentes — pero no persigue el fotorrealismo: su dirección de arte es la composición estilizada, con PBR de criterio y acabado toon/cartoon, poniendo la legibilidad y el estilo por encima del realismo.

## Requisitos

- **Rust 1.80 o superior** (edition 2021).
- **Driver Vulkan 1.3**. En Linux sin GPU sirve el render por software (lavapipe: paquete `mesa-vulkan-drivers` y `libxkbcommon-x11-0`).
- **Runtime de .NET 8** para ejecutar los scripts C# de las escenas.
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

## Estado del proyecto

GlassEnginer **0.1.0** es una beta en desarrollo activo. El repositorio está publicado, la CI está en verde y los **382 tests automatizados** pasan correctamente.

## Licencia

Licencia dual **MIT OR Apache-2.0**. Consulta el fichero `LICENSE` en la raíz del repositorio.