# Complementos (plugins) de GlassEnginer

`catalog.json` es el catálogo oficial que lee el Launcher: por cada
complemento indica su `id`, su nombre visible, su versión, la URL del
`.zip` en GitHub Releases y su SHA-256, que el Launcher verifica antes
de instalar.

## MeshForge (id interno `herramienta-3d`)

Mini modelador del editor: edición de mallas `.glb` con operadores,
booleanas Manifold, selección sub-objeto y horneado con receta. Panel
`MeshTool` y tools MCP `glass_mesh_*`.

- Desde la **v1.0.1** el nombre visible del complemento es **MeshForge**
  (antes «Herramienta 3D»): pestaña del editor, Complementos del
  Launcher y mensajes al usuario.
- El **id interno se mantiene `herramienta-3d`** por compatibilidad con
  las instalaciones existentes: clave en `plugins.json`, carpeta de
  instalación y puerta `plugin_enabled("herramienta-3d")`. No cambia el
  crate `glass_mesh_edit`, ni el enum `EditorTab::MeshTool`, ni las
  tools `glass_mesh_*`.
