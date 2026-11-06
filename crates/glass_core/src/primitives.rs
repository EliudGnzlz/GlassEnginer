//! Dimensiones canónicas de las primitivas integradas del motor.
//!
//! Viven en `glass_core` porque las consumen DOS crates que no deben
//! depender entre sí: `glass_renderer` (los generadores de malla en
//! `mesh.rs` construyen exactamente estas medidas) y `glass_physics`
//! (`collider_shape_for_builtin` deriva el collider de la misma forma a
//! partir de la escala mundial). Si un día cambia una medida, cambia
//! aquí y AMBOS lados se mueven a la vez; sin esta fuente única, la
//! inferencia de colliders quedaría acoplada a números mágicos del
//! generador y un cambio de malla rompería la física en silencio.
//!
//! Convención (la de las primitivas de Unity): la esfera y el cilindro
//! miden 1 de diámetro; la cápsula y el cilindro, 2 de alto total; todo
//! en espacio de objeto, centrado en el origen, eje principal +Y.

/// Radio de la esfera integrada (diámetro 1).
pub const SPHERE_RADIUS: f32 = 0.5;
/// Radio de la cápsula integrada.
pub const CAPSULE_RADIUS: f32 = 0.5;
/// Mitad de la altura del TRAMO RECTO de la cápsula (sin casquetes):
/// la altura total es 2 · (esto + el radio) = 2.
pub const CAPSULE_CYLINDER_HALF_HEIGHT: f32 = 0.5;
/// Radio del cilindro integrado (diámetro 1).
pub const CYLINDER_RADIUS: f32 = 0.5;
/// Mitad de la altura total del cilindro (altura total 2).
pub const CYLINDER_HALF_HEIGHT: f32 = 1.0;
