# GlassEngine AI Orchestration & Roles

Este documento rige la arquitectura, el flujo de trabajo y la delegación entre agentes de Inteligencia Artificial para el desarrollo de **GlassEngine**.

## 1. Roles y Responsabilidades

*   **Lead Orchestrator (Gemini Argon High / CLI):**
    *   Gestiona el contexto global del documento de diseño (`SDD_GlassEngine.md`).
    *   Coordina y valida cada hito de desarrollo.
    *   Aplica controles estrictos anti-sobreingeniería antes de cada integración o merge.
    *   Despacha subtareas especializadas mediante MCP.

*   **Core / Low-Level Agent (DeepSeek v4.1 via MCP):**
    *   Desarrollo exclusivo de Rust de bajo nivel.
    *   Llamadas directas a Vulkan a través de la crate `ash`.
    *   Gestión manual y segura de memoria GPU con `gpu-allocator`.
    *   Fronteras FFI sin sobrecarga (`#[repr(C)]`, punteros opacos).
    *   Subproceso de audio desacoplado.

*   **UI / Tooling Agent (Kimi via MIRAI MCP):**
    *   **REGLA OBLIGATORIA:** Todas las llamadas a Kimi (v3.1 Pro / v3.2 Pro) se realizan estrictamente a través de la herramienta `ask_mirai` del servidor MCP `mirai`. Queda deshabilitado el MCP local antiguo.
    *   Diseño y layout del editor en `egui` (estilo Unity con paneles desacoplados).
    *   Puente de scripting C# (.NET CoreCLR / `netcorehost`).
    *   Scripts headless de Blender en Python para exportación `.blend` -> `.glb`.
    *   Servidor MCP interno del motor para inspección en tiempo de ejecución.

## 2. Directivas Anti-Sobreingeniería

1.  **Memoria Vulkan:** Usar siempre `gpu-allocator` junto con `ash`. Prohibido crear un asignador de memoria personalizado desde cero.
2.  **Grafo de Escena:** Evitar frameworks ECS hipercomplejos inicialmente. Utilizar un grafo de Entidad-Componente jerárquico simple para mapeo 1:1 transparente con C#.
3.  **Puente C#:** Interfaz plana C-ABI con `#[repr(C)]` y punteros opacos. Evitar wrappers basados en reflexión pesada.
4.  **Recarga en caliente de Blender:** Delegar la conversión a un proceso de fondo `blender -b input.blend --python export.py` exportando a `.glb` estándar, procesado con la crate `gltf`.
5.  **Servidor MCP del Motor:** Implementación ligera JSON-RPC sobre `stdio` o `HTTP localhost`.

## 3. Herramientas Especiales del Motor (`tools/`)

*   `ast_patcher.py`: Reemplazo quirúrgico de funciones para evitar reescritura masiva de archivos.
*   `cargo_diagnostics.py`: Extracción concisa de errores de `cargo check --message-format=json`.
*   `ffi_validator.py`: Validación de alineación de memoria entre structs de Rust y C#.
*   `vulkan_grep.py`: Búsqueda semántica en cabeceras locales `/usr/include/vulkan/` para evitar alucinaciones de API.
*   `headless_blender_bridge.py`: Conversión y validación de cabeceras binarias glTF.
