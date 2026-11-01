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
| **Hito 6** | Pipeline de Blender y Recarga en Caliente | **En Progreso** | - | Kimi (Mirai) + DeepSeek |
| **Hito 7** | Entorno de Scripting en C# (.NET CoreCLR) | Pendiente | - | Kimi (Mirai) + DeepSeek |
| **Hito 8** | Subsistema de Audio 2D/3D Desacoplado (Kira) | Pendiente | - | DeepSeek |
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
  * **Kimi (vía Mirai):** Visto bueno técnico condicionado con directrices registradas para Hito 6 (aislamiento de asset cache fuera del lock de escena, staging buffers con fences para nueva geometría).

---

### Hito 6: Pipeline de Blender y Recarga en Caliente (En Progreso)
* **Objetivo:** Automatizar la importación de modelos `.blend` y `.glb` mediante Blender en modo headless y habilitar recarga en caliente (hot-reloading) con el crate `notify`.
* **Capacidades a Implementar:**
  * Vigilante de directorio `assets/` en un hilo dedicado con `notify 8.0`.
  * Conversor desatendido: si se añade o modifica un archivo `.blend`, se invoca `blender -b input.blend --python tools/export_gltf.py` para exportar a `.glb`.
  * Cargador de mallas glTF en `glass_renderer` usando el crate `gltf 1.4` (extracción de posiciones, normales, coordenadas UV e índices).
  * Creación de Vertex Buffers e Index Buffers en VRAM con `gpu-allocator 0.28` vía Staging Buffer con barreras y fences.
  * Actualización atómica en el `AssetCache` de la escena sin detener el render loop.
