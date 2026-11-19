//! Tipos de datos compartidos entre el Hub y el motor: las
//! plantillas de proyecto nuevo y los ejemplos (`--sample`). Los
//! define el lanzador pero el motor también los consume (la
//! creación de proyectos y el receptor de `--sample` viven en el
//! binario del motor), por eso viven en la infraestructura común.

use egui::Color32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTemplate {
    Platformer3D,
    PhysicsSandbox,
    EmptyScene,
}

impl ProjectTemplate {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Platformer3D => "3D Platformer (Rapier3D + CoreCLR)",
            Self::PhysicsSandbox => "3D Physics Playground (Rapier3D)",
            Self::EmptyScene => "Empty 3D Scene (Escena Vacía)",
        }
    }

    /// Nombre corto para la tarjeta de plantilla, como en el Hub viejo.
    pub fn short_name(&self) -> &'static str {
        match self {
            Self::Platformer3D => "Plataformas 3D",
            Self::PhysicsSandbox => "Físicas 3D",
            Self::EmptyScene => "Escena vacía",
        }
    }

    pub fn short_desc(&self) -> &'static str {
        match self {
            Self::Platformer3D => "Personaje 3D con físicas y script C#.",
            Self::PhysicsSandbox => "Cubos rígidos: fricción y rebote.",
            Self::EmptyScene => "Escena 3D limpia: cámara y luz solar.",
        }
    }

    pub fn glyph(&self) -> &'static str {
        match self {
            Self::Platformer3D => "🧊",
            Self::PhysicsSandbox => "🎲",
            Self::EmptyScene => "⬜",
        }
    }

    pub fn tile_color(&self) -> Color32 {
        match self {
            Self::Platformer3D => Color32::from_rgb(33, 150, 243),
            Self::PhysicsSandbox => Color32::from_rgb(255, 140, 0),
            Self::EmptyScene => Color32::from_rgb(130, 140, 150),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleScenePreset {
    PlatformerDemo,
    PhysicsPlayground,
    AudioLightingShowcase,
    FullSampleGame,
}

