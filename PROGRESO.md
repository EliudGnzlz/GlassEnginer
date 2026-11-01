# Registro de Progreso y Arquitectura: GlassEngine

Este documento registra en tiempo real cada paso, decisión arquitectónica, implementación técnica y validación cruzada entre agentes de IA durante el desarrollo de **GlassEngine**.

---

## Estado Global del Proyecto

| Hito | Nombre | Estado | Verificación | Agente Asignado |
| :--- | :--- | :---: | :---: | :--- |
| **Hito 1** | Workspace Cargo & Contratos Modulares | **Completado** | 100% | Gemini (Orquestador) |
| **Hito 2** | Ventana Nativa y Contexto Vulkan Multi-GPU | **Completado** | 100% | DeepSeek (Bajo Nivel) |
| **Hito 3** | Swapchain, Render Passes y Viewport Offscreen | **Completado** | 100% | DeepSeek + Kimi (Mirai) |
| **Hito 4** | Editor Docking estilo Unity en egui | **Completado** | 100% | Kimi (Mirai) + DeepSeek |
| **Hito 5** | Servidor MCP Embebido en Tiempo de Ejecución | **Completado** | 100% | Kimi (Mirai) + DeepSeek |
| **Hito 6** | Pipeline de Blender y Recarga en Caliente | **Completado** | 100% | DeepSeek + Kimi (Mirai) |
| **Hito 7** | Entorno de Scripting en C# (.NET CoreCLR) | **Completado** | 100% | Kimi (Mirai) + DeepSeek |
| **Hito 8** | Subsistema de Audio 2D/3D Desacoplado (Kira) | **En Progreso** | - | DeepSeek + Kimi (Mirai) |
| **Hito 9** | Simulación Física 3D (Rapier3D) | Pendiente | - | DeepSeek |
| **Hito 10**| Compilación Dual (Editor vs Standalone) | Pendiente | - | Gemini + DeepSeek |

---

## Detalle de Hitos Realizados

### Hito 1: Workspace Cargo & Contratos Base
* **Fecha:** 2026-11-01
* **Acciones:**
  * Estructuración modular en crates (`glass_core`, `glass_renderer`, `glass_editor`, `glass_scripting`, `glass_mcp`, `glass_app`).
  * Creación de herramientas quirúrgicas en `tools/` (`ast_patcher.py`, `cargo_diagnostics.py`, `ffi_validator.py`, `vulkan_grep.py`, `headless_blender_bridge.py`).
  * Registro de la skill global `glassengine-tooling`.
* **Resultado:** Compilación completa y limpia con `cargo check --workspace` y ejecución de prueba en `glass_app`.

### Hito 2: Ventana Nativa y Contexto Vulkan Multi-GPU
* **Fecha:** 2026-11-01
* **Acciones:**
  * Integración con `winit 0.30` y `raw-window-handle 0.6`.
  * Carga dinámica de `ash 0.38` y creación de `VkSurfaceKHR` mediante `ash-window 0.13`.
  * Activación y canalización de `VK_LAYER_KHRONOS_validation` hacia el logger de Rust.
  * **Mejora Multi-GPU universal:** Algoritmo de puntuación agnóstico que soporta y clasifica cualquier GPU (NVIDIA, AMD Radeon, Intel Arc / Iris / UHD, GPUs virtuales y rasterizadores de CPU) garantizando compatibilidad en cualquier equipo.
* **Resultado:** Inicialización exitosa de GPU, colas de gráficos y presentación, y destrucción segura en orden inverso sin fugas de memoria.

### Hito 3: Swapchain, Render Passes y Viewport Offscreen
* **Fecha:** 2026-11-01
* **Acciones:**
  * Implementación de `SwapchainContext` con `ash::khr::swapchain::Device`, modos `MAILBOX` y `FIFO`.
  * Implementación de `OffscreenTarget` gestionando texturas de color (`R8G8B8A8_UNORM`, `SAMPLED`) y profundidad (`D32_SFLOAT`) asignadas mediante `gpu-allocator 0.28`.
  * RenderPass offscreen desacoplado que transiciona la imagen de color a `SHADER_READ_ONLY_OPTIMAL` para muestreo directo en UI.
  * Pipeline gráfico con shaders compilados en SPIR-V (`triangle.vert.spv` y `triangle.frag.spv`) con depth-testing activado.
  * Validación en hardware real (NVIDIA GTX 1070) con código de salida 0.
* **Auditoría Cruzada:** Aprobado con Visto Bueno técnico por Kimi (vía Mirai MCP).

### Hito 4: Editor Docking estilo Unity en egui
* **Fecha:** 2026-11-01
* **Acciones:**
  * Integración de `egui 0.36`, `egui-winit 0.36` y `egui-ash-renderer 0.13` sobre `ash 0.38`.
  * Asignador de memoria VRAM compartido: `gpu-allocator 0.28` encapsulado en `Arc<Mutex<Allocator>>` compartido limpiamente entre `GlassRenderer` y `egui-ash-renderer`.
  * Implementación de los 4 paneles principales de docking estilo Unity:
    * **Hierarchy (Izquierda):** Árbol de GameObjects interactivo sincronizado con el estado real de la escena.
    * **Inspector (Derecha):** Inspección y manipulación en tiempo real de componentes Transform (Posición, Rotación, Escala con widgets `DragValue`), Material Albedo con selector de color y componente Script C#.
    * **Console & Project Browser (Abajo):** Pestaña de consola con filtrado selectivo (Info, Warn, Error) y pestaña de explorador de assets del proyecto (`assets/`).
    * **3D Viewport (Centro):** Muestreo directo en GPU mediante `egui::TextureId::User(0)` vinculado a un `VkDescriptorSet` que apunta a la textura offscreen en VRAM (Zero-copy en CPU). Barra de herramientas con modos Play/Pause y selección de Gizmos (Translate, Rotate, Scale).
  * RenderPass dedicado de composición sobre el Swapchain a 1280x720. El redimensionamiento de paneles docking ocurre en egui sin recrear innecesariamente el Swapchain Vulkan.
  * Verificación en hardware real NVIDIA GTX 1070: frame renderizado, presentación en swapchain y salida limpia con código 0 y 0 advertencias en validation layers.
* **Auditoría Cruzada:**
  * **DeepSeek:** Visto bueno a la arquitectura de descriptor sets, pipelines y separación de ciclo de vida del swapchain.
  * **Kimi (vía Mirai):** Visto bueno técnico confirmando la solidez de la UI y la estrategia zero-copy.

### Hito 5: Servidor MCP Embebido en Tiempo de Ejecución
* **Fecha:** 2026-11-01
* **Acciones:**
  * Creación del subsistema `glass_mcp` con protocolo estándar Model Context Protocol (especificación 2024-11-05).
  * Estructura de estado `SceneState` protegida mediante `Arc<Mutex<SceneState>>`, garantizando thread-safety entre agentes MCP externos y el ciclo de renderizado.
  * Implementación completa de las 5 herramientas operativas:
    * `glass_get_scene_hierarchy`: Inspecciona el grafo de escena completo, entidades y componentes.
    * `glass_create_entity`: Crea entidades en tiempo real y las registra en la jerarquía.
    * `glass_modify_component`: Modifica componentes Transform (posición, rotación, escala) y Material (color Albedo, wireframe), impactando de inmediato en el Inspector y el Viewport.
    * `glass_capture_viewport`: Captura el frame actual del Viewport 3D en formato PNG codificado en Base64 sin detener la GPU.
    * `glass_reload_shaders`: Dispara la recarga en caliente de los shaders SPIR-V.
  * Soporte de modo dual: interactivo en la app principal y modo headless stdio (`--mcp` / `--mcp-stdio`) para agentes de IA externos.
  * Suite de pruebas unitarias (`cargo test -p glass_mcp`) con 100% de éxito.
  * Verificación en hardware real NVIDIA GTX 1070 con código de salida 0 y 0 errores en validation layers.
* **Auditoría Cruzada:**
  * **Kimi (vía Mirai):** Visto bueno técnico condicionado con directrices registradas para Hito 6.

### Hito 6: Pipeline de Blender y Recarga en Caliente
* **Fecha:** 2026-11-01
* **Acciones:**
  * Integración con Blender 5.2.2 LTS desatendido (`headless_blender_bridge.py`) convirtiendo archivos `.blend` nativos a `.glb` binarios validados.
  * Cargador de mallas glTF (`gltf 1.4`): extracción eficiente de posiciones, normales, coordenadas UV e índices de mallas 3D (validado con modelo de cubo estándar de 24 vértices y 36 índices).
  * Sistema de transferencia a GPU con Staging Buffers (`MemoryLocation::CpuToGpu`) hacia VRAM (`MemoryLocation::GpuOnly`) mediante `vkCmdCopyBuffer` y sincronización segura con fences y `gpu-allocator 0.28`.
  * Pipeline de renderizado 3D indexado (`mesh.vert.spv` + `mesh.frag.spv`) con vertex layout de 32 bytes y PushConstants de 64 bytes para matriz Model-View-Projection (MVP) con rotación continua en tiempo real.
  * Implementación de `AssetCache` (`Arc<RwLock<AssetCache>>`) con sustitución atómica de mallas en VRAM desacoplado del mutex de escena para evitar stalls.
  * Implementación de `AssetWatcher` en hilo secundario mediante `notify 8.2` vigilando el árbol `assets/` para detectar y recargar automáticamente modelos modificados.
  * Validación en hardware real NVIDIA GTX 1070 con código de salida 0 y 0 advertencias de validación Vulkan.
* **Auditoría Cruzada:**
  * **DeepSeek:** Visto bueno condicionado, validando topología estándar, Staging Buffers y push constants. Directrices anotadas para Hito 7: aislamiento de buffers y concurrencia blindada.

### Hito 7: Entorno de Scripting en C# (.NET CoreCLR)
* **Fecha:** 2026-11-01
* **Acciones:**
  * Creación y compilación limpia de la biblioteca gestionada `GlassEngine.Core` (.NET 8):
    * `Structs.cs`: `Vector3`, `Quaternion`, `TransformData` con `[StructLayout(LayoutKind.Sequential)]` (tamaño 40 bytes exactos, alineación 4 bytes).
    * `NativeDelegates.cs`: Delegados Cdecl `GetTransformDelegate`, `SetTransformDelegate`, `LogMessageDelegate`.
    * `GlassBehaviour.cs`: Clase base de scripting con ciclo de vida `Awake()`, `StartIfNeeded()`, `Update(float deltaTime)` y propiedades sincronizadas `Position`, `Rotation`, `Scale`.
    * `RuntimeBridge.cs`: Métodos `[UnmanagedCallersOnly]` (`InitBridge`, `CreateScriptInstance`, `UpdateScriptInstance`, `DestroyScriptInstance`) y resolución dinámica de tipos.
    * Scripts de juego de ejemplo: `Rotator.cs` (rotación suave continua en eje Y) y `PlayerMovement.cs` (movimiento sinusoidal en X y Z).
  * Lado Rust (`crates/glass_scripting`):
    * `bindings.rs`: `TransformFfi` `#[repr(C)]` con aserciones en tiempo de compilación y validación cruzada con `tools/ffi_validator.py`. Callbacks nativos `get_transform`, `set_transform`, `log_message` con comprobación de punteros nulos y canalización a logs de Rust.
    * `bridge.rs`: Trait `SceneBridge` y registro global thread-safe (`register_scene_bridge`).
    * `host.rs`: Carga de `libhostfxr.so` vía `netcorehost 0.22`, arranque con `GlassEngine.Core.runtimeconfig.json`, resolución de delegados sin sobrecosto de reflexión y gestión de ciclo de vida de instancias.
  * Integración con el motor (`crates/glass_app`):
    * Implementación de `EngineSceneBridge` sincronizando transforms de entidades de `SceneState` de forma bidireccional mediante `Arc<Mutex<SceneState>>`.
    * Invocación de `script_host.update_all(dt)` en el bucle principal.
  * Verificación:
    * Suite unitaria `crates/glass_scripting/tests/coreclr_test.rs`: 100% aprobada en 0.08s.
    * Prueba de integración en hardware real (`cargo run -p glass_app -- --test-m7`): instanciación, ejecución de `Awake`, `Start`, múltiples ticks de `Update`, mutación de posición y rotación verificadas, y renderizado offscreen en NVIDIA GTX 1070 con código 0 y 0 errores.
* **Auditoría Cruzada:**
  * **Kimi (vía Mirai MCP):** Visto bueno técnico (Sign-off) otorgado, destacando la solidez de `[UnmanagedCallersOnly]` y la precisión del layout de 40 bytes. Directrices anotadas para Hito 8: bindings C# para `AudioSource`, `AudioListener` y `AudioClip`.
  * **DeepSeek:** Visto bueno técnico (Sign-off) otorgado, validando la ausencia de UB en FFI, las aserciones de compilación de `TransformFfi` y la carga determinista de hostfxr. Directrices de bajo nivel anotadas para Hito 8: principio de hilo de audio no bloqueante y canales SPSC sin alocación.

---

### Hito 8: Subsistema de Audio 2D/3D Desacoplado (Kira) (En Progreso)
* **Objetivo:** Implementar un subsistema de audio espacializado e interactivo basado en `kira` ejecutándose en un hilo de audio de alta prioridad totalmente desacoplado del hilo de render y de la simulación, expuesto tanto a C# como a la escena.
* **Directrices de Implementación:**
  * Hilo de audio dedicado sin bloqueos, sin alocaciones en el callback y sin I/O.
  * Canal lock-free SPSC (`ringbuf`) para encolar comandos desde simulación/C# (`Play`, `Stop`, `SetVolume`, `SetPosition`).
  * Componentes `AudioSource` y `AudioListener` vinculados a las entidades y a la posición de la cámara.
  * Exposición a C# en `GlassEngine.Core`.
