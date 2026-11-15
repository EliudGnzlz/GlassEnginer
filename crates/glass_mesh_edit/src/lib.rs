//! # glass_mesh_edit
//!
//! Núcleo geométrico de edición de malla de GlassEnginer (base del futuro
//! mini-modelador). Este crate contiene solo los datos y algoritmos:
//! soldadura, adyacencia, validación manifold, estadísticas y horneado.
//! Los operadores-comando y la receta viven en otro paquete.
//!
//! ## La EditMesh
//!
//! [`EditMesh`] es la malla topológica de **sesión**. Los vértices que
//! comparten posición (dentro de una tolerancia) se sueldan en un único
//! vértice lógico, y las caras se expresan sobre esos vértices lógicos.
//! Así una arista del cubo es una sola arista con dos caras incidentes,
//! no dos aristas sueltas como en la malla de render.
//!
//! ## Ciclo de edición
//!
//! 1. **Entrar**: [`EditMesh::from_mesh_data`] convierte una [`MeshData`]
//!    de render (vértices duplicados por cara) en una `EditMesh` soldada.
//!    El resultado de la soldadura queda en [`EditMesh::last_weld`].
//! 2. **Editar**: los operadores mutan `verts`/`faces`. Toda mutación marca
//!    la adyacencia como sucia y se reconstruye de forma perezosa en la
//!    siguiente consulta ([`EditMesh::edge_faces`],
//!    [`EditMesh::validate_manifold`], [`EditMesh::stats`]).
//! 3. **Hornear**: [`EditMesh::bake`] produce de nuevo una [`MeshData`]
//!    lista para GPU y para exportar a `.glb`.
//!
//! ## Decisión de horneado: normales planas
//!
//! El horneado expande un vértice por esquina de cara y asigna a las tres
//! esquinas la normal de su cara. Se descarta el suavizado porque las mallas
//! del modelador son de estilo low-poly con aristas duras: promediar normales
//! en un cubo soldado redondearía el sombreado. El coste es memoria
//! (3 vértices por triángulo), aceptable para mallas de edición. Las UVs se
//! heredan del vértice lógico; si no existen, se generan por proyección
//! sobre el eje dominante de la normal de cada cara.

pub mod edit_mesh;
pub mod mesh_data;

pub use edit_mesh::{EditMesh, ManifoldReport, MeshStats, WeldReport};
pub use mesh_data::{MeshData, MeshEditError};
