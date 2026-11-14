# Registro — Verificación en vivo de prefabs desde el editor (2026-11-14)

**Resultado: la creación de prefabs desde el editor FUNCIONA en vivo. El bloque
de «crear prefabs desde el editor» queda CANCELADO (decisión de D3m3tr10).**

## Qué se ejerció, paso a paso

1. Editor real abierto (build con feature `editor`) con la escena `Plat3D_L2`.
   En la Hierarchy se seleccionó **Enemigo Suelo Este** (id 126): raíz con
   MeshRenderer, RigidBody & Collider (Kinematic) y Script C#
   (`Plat3DEnemy`), con 2 hijos (Luz Nocturna con luz + script propio y
   Núcleo Luminoso con malla).
2. Menú **Assets → «📦 Save Selection as Prefab»**: el editor mostró
   «Prefab guardado exitosamente en assets/prefabs/Enemigo_Suelo_Este.prefab.json»
   y el archivo apareció en disco (4.890 B).
3. Contenido del `.prefab.json` verificado: raíz con `mesh`, `physics`
   (body_type Kinematic), `script` + `script_data` con sus campos propios
   (`move_speed 4.2`, `jump_velocity 7.5`, `auto_demo false`) y los 2
   descendientes completos con sus `parent` (el hijo con luz lleva su
   `light` y su `script`/`script_data`; el otro, su `mesh` «sphere»).
   El subárbol no tenía ningún `animator` que capturar (los enemigos de
   Plat3D_L2 no llevan): el formato clona el `EntityData` entero
   (`Prefab::from_entity`), así que el campo viaja por la misma vía.
4. **Instanciación desde el panel Project**: doble clic sobre
   `Enemigo_Suelo_Este.prefab.json` → «Instanciación de prefab encolada» y
   una segunda copia del enemigo apareció en la Hierarchy como entidad
   **#138**, seleccionada, en (0, 1, 0), con sus secciones de Mesh
   Renderer, RigidBody & Collider y Script. La escena `Plat3D_L2` en
   disco NO se modificó (la sesión se cerró sin guardar).
5. **Instanciación desde C#** (escena `PrefabTest`, script
   `PrefabEditorTest`): `GameObject.InstantiatePrefab("Enemigo_Suelo_Este",
   (0, 1.5, 0))` → raíz 41; entidades 4 → **7** (raíz + 2 hijos), cuerpos
   1 → **2** (la raíz Kinematic), colliders 1 → 2.
   **[PREFABTEST] GLOBAL OK.**

## Incidencias del entorno (no del motor)

- La primera sesión «sin editor» fue la **trampa del binario compartido**:
  `target/debug/glass_app` era la build de juego (`--no-default-features`
  del pipeline local sobrescribe la del editor). Reconstruida la build del
  editor, el Launcher y los paneles aparecieron.
- La primera corrida del paso de C# no instanció el script de prueba
  porque la DLL era vieja: compilar el C# desde el directorio del proyecto
  no recompila (hay que compilar desde `~/workspace` con el SDK completo).
  Diagnóstico por `strings` sobre la DLL (0 apariciones de la clase → 1
  tras compilar bien).

Consumo del bloque: 0 tokens de APIs (verificación directa de Rayo).
