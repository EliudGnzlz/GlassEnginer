use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScriptError {
    #[error("CoreCLR initialization failed: {0}")]
    InitFailed(String),
    #[error("Assembly not found: {0}")]
    AssemblyNotFound(String),
}

pub struct ScriptHost {
    pub is_initialized: bool,
}

impl ScriptHost {
    pub fn new() -> Self {
        Self { is_initialized: false }
    }

    pub fn initialize(&mut self) -> Result<(), ScriptError> {
        log::info!("Inicializando Host de Scripting C# (.NET CoreCLR)");
        self.is_initialized = true;
        Ok(())
    }
}
