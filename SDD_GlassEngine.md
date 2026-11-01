# Software Design Document (SDD) & Orchestration Plan: GlassEngine

## 1. Orchestration Directives & AI Roles

*   **Lead Orchestrator:** Gemini (Argon / CLI). Manages the global SDD context, coordinates tasks, enforces anti-overengineering checks before merges, and dispatches sub-tasks via MCP.
*   **Core/Low-Level Agent (MCP):** DeepSeek (v4.1). Exclusively handles low-level Rust, direct Vulkan calls (`ash`), manual GPU memory management (`gpu-allocator`), raw FFI boundaries, and the audio thread.
*   **UI/Tooling Agent (MCP):** Kimi (v3.1 Pro). Handles the `egui` editor layout (Unity-style), C# scripting bridge (`netcorehost`/CoreCLR), Python Blender scripts, and the internal MCP server.

## 2. Anti-Overengineering Guidelines

1.  **Vulkan Memory:** Do not write a custom memory allocator from scratch; use the battle-tested `gpu-allocator` crate alongside `ash`.
2.  **Scene Architecture:** Avoid complex ECS frameworks (like `bevy_ecs`) initially. Use a simple, hierarchical Entity-Component graph to ensure a painless 1:1 mapping with C# objects.
3.  **C# Bridge:** Avoid heavy, reflection-based wrappers. Use flat C-ABI interfaces with opaque pointers and `#[repr(C)]` structs.
4.  **Blender Hot-Reload:** Do not parse native `.blend` files. Delegate conversion to a background headless Blender process (`blender -b --python`) exporting to standard `.glb`, then parse with the `gltf` crate.
5.  **MCP Server:** Implement the engine's MCP server using lightweight JSON-RPC over `stdio` or `HTTP localhost` without unnecessary middleware.

## 3. Custom AI Skills & Tooling (Adapted & Optimized)

Inspired by high-efficiency AI workflows (Caveman, Karpathy-skills, Entroly, everything-claude-code), these skills must be registered in the orchestrator to prevent full-file rewrites and context bloat.

*   **`skill_ast_patcher` (Adapted from Claude/Karpathy tools):** Never rewrite a full file for a small change. Use sed/awk or AST-aware patching to replace specific function blocks in Rust/C#.
*   **`skill_cargo_diagnostics`:** Runs `cargo check --message-format=json`. Instead of dumping raw output, it parses the JSON, extracts the exact file/line/column of the Rust compiler error, and feeds only the relevant struct/lifetime failure to DeepSeek.
*   **`skill_ffi_validator` (Custom for GlassEngine):** Cross-references Rust `#[repr(C)]` structs with C# `[StructLayout(LayoutKind.Sequential)]` definitions to ensure memory alignment and prevent segfaults before compilation.
*   **`skill_vulkan_grep`:** A semantic search tool hooked into the local Vulkan SDK headers (`vulkan_core.h`) or `ash` docs. Prevents the AI from hallucinating struct members or command buffer functions.
*   **`skill_headless_blender_bridge`:** Executes the Python export script in isolation and parses the resulting `.glb` binary header to verify vertex/index counts without needing the UI.

## 4. 10-Point Development Plan (Verifiable Milestones)

### Milestone 1: Cargo Workspace & Base Contracts (Spec First)
*   **Goal:** Define the modular workspace and shared traits without heavy logic.
*   **Modules:** `glass_core` (Window/Events), `glass_renderer` (Vulkan), `glass_editor` (UI), `glass_scripting` (C# Bridge), `glass_mcp` (Server).
*   **Assignment:** **Gemini** generates the folder structure; **DeepSeek** validates Rust compiler flags.

### Milestone 2: Native Window & Vulkan Context
*   **Goal:** Spawn a functional window and link the physical GPU.
*   **Tech:** `winit` for OS events. `ash::Entry`, `Instance` (with `VK_LAYER_KHRONOS_validation`). Select `PhysicalDevice` and create `LogicalDevice` with dedicated Graphics/Present queues.
*   **Assignment:** **DeepSeek**.

### Milestone 3: Swapchain, Render Passes & Offscreen Viewport
*   **Goal:** Draw basic geometry to an offscreen texture (Framebuffer) rather than directly to the screen, preparing for the editor UI.
*   **Tech:** `Swapchain`, `Images`, `ImageViews`. Decoupled Render Pass for 3D scene rendering to a sampleable `VkImage`. Basic SPIR-V pipeline with depth-testing.
*   **Assignment:** **DeepSeek**.

### Milestone 4: Unity-Style Docking Editor (egui)
*   **Goal:** Build a familiar GUI layout.
*   **Panels:** 
    *   *Hierarchy (Left):* Scene entity tree.
    *   *Scene Viewport (Center):* Embeds the Vulkan texture from M3 with orbital camera controls.
    *   *Inspector (Right):* Component editing (`Transform`, etc.).
    *   *Project/Console (Bottom):* File explorer and logger.
*   **Assignment:** **Kimi**.

### Milestone 5: Embedded GlassEngine MCP Server
*   **Goal:** Allow external agents to inspect/modify the running engine.
*   **Tools:** `glass_get_scene_hierarchy()`, `glass_create_entity()`, `glass_modify_component()`, `glass_capture_viewport()`.
*   **Assignment:** **Kimi** (JSON-RPC logic) and **DeepSeek** (thread-safe execution in Rust).

### Milestone 6: Blender Pipeline & Hot-Reloading
*   **Goal:** Auto-sync `.blend` file saves to the engine viewport.
*   **Tech:** `notify` crate watches `Assets/`. Triggers `blender -b input.blend --python export.py`. Async glTF loading and in-place `VkBuffer` updates in VRAM.
*   **Assignment:** **Kimi** (Python script) and **DeepSeek** (Vulkan buffer/descriptor management).

### Milestone 7: C# Scripting Environment (CoreCLR)
*   **Goal:** Write game logic in C# using a component-oriented syntax.
*   **Tech:** Embed CoreCLR via `netcorehost`. Base `EntityBehaviour` class (`OnStart`, `OnUpdate`). Flat FFI ports (`extern "C"`) for `Transform` and `Input` manipulation.
*   **Assignment:** **Kimi** (C# project/bindings) and **DeepSeek** (Rust FFI layer).

### Milestone 8: 2D/3D Audio Subsystem
*   **Goal:** Stereo and spatialized audio on a decoupled thread.
*   **Tech:** `kira` or `cpal` running independently to prevent audio stuttering during FPS drops. `AudioListener` and `AudioSource` components. Exposed to C#.
*   **Assignment:** **DeepSeek** (Audio backend) and **Kimi** (C# bindings).

### Milestone 9: 3D Physics Simulation (Rapier3D)
*   **Goal:** Rigid body dynamics and collision detection.
*   **Tech:** Integrate `rapier3d`. `RigidBody` and `Collider` components mapped to `egui`. Fixed timestep loop synchronizing transform matrices back to C# and the renderer.
*   **Assignment:** **DeepSeek**.

### Milestone 10: Dual Compilation (Editor vs. Standalone)
*   **Goal:** Output lightweight binaries without the editor/MCP overhead.
*   **Tech:** Feature flags (`#[cfg(feature = "editor")]`). Standalone mode renders directly to the swapchain fullscreen. Binary asset packing.
*   **Assignment:** **Gemini** (Cargo profiles) and **DeepSeek** (conditional dependencies).

## 5. Bootstrapping Instructions for Gemini CLI

When starting the CLI session:
1.  Set the orchestration model (`model set argon`).
2.  Load this document (`SDD_GlassEngine.md`) into the core context.
3.  Load/Register the custom skills defined in Section 3 (`skill_ast_patcher`, `skill_cargo_diagnostics`, etc.).
4.  Execute tasks sequentially, strictly completing **Milestone 1** before writing any Vulkan/GPU code.