//! Motor de documentos de la UI (fase UI/Canvas): el documento se
//! DERIVA de la escena (los componentes Unity son la fuente), con
//! estilos tipados en cascada, layout en dos pasadas y pintor sobre el
//! lote de quads del motor. Este crate no depende de Vulkan ni de
//! egui: entran la escena y el viewport, salen rects y quads.
//!
//! Sub-bloques: V1 frontera y migración (este), V2 núcleo/pintor, V3
//! texto SDF, V4 autorado, V5 grupo/recortes, V6 interacción, V7
//! Camera/World. Diseño: `diseno_fase_ui_canvas.md` en la raíz del
//! proyecto de documentación del motor.

pub mod derivacion;
pub mod documento;
pub mod estilos;
pub mod migracion;
