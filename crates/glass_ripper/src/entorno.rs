//! Integración de la importación con el entorno del motor: cómo
//! encontrar la fachada C# para la garantía de compilación de los
//! scripts y el resumen corto del informe para la interfaz y el MCP.

use std::path::PathBuf;

use crate::compilacion::verificar_compilacion;
use crate::informe::InformeImportacion;
use crate::scripts::VerificadorCompilacion;
use crate::EstadoInforme;

/// Busca el `GlassEngine.Core.csproj` de la fachada Unity, en este
/// orden: la variable `GLASS_FACADE_CSPROJ`; junto al ejecutable
/// (`<exe>/../../managed/…` es el árbol de desarrollo y
/// `<exe>/../managed/…` una instalación); y bajo el directorio de
/// trabajo (`managed/…` al arrancar desde la raíz del repo).
pub fn buscar_fachada() -> Option<PathBuf> {
    if let Ok(ruta) = std::env::var("GLASS_FACADE_CSPROJ") {
        let p = PathBuf::from(ruta);
        if p.is_file() {
            return Some(p);
        }
    }
    let mut candidatos: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidatos
                .push(dir.join("../../managed/GlassEngine.Core/GlassEngine.Core.csproj"));
            candidatos.push(dir.join("../managed/GlassEngine.Core/GlassEngine.Core.csproj"));
            candidatos.push(dir.join("managed/GlassEngine.Core/GlassEngine.Core.csproj"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidatos.push(cwd.join("managed/GlassEngine.Core/GlassEngine.Core.csproj"));
    }
    candidatos.into_iter().find(|p| p.is_file())
}

/// Crea el verificador de compilación para los scripts si el
/// entorno lo permite: hace falta la fachada y un `dotnet`
/// ejecutable (se sondea con `dotnet --version`; la variable
/// `GLASS_DOTNET` puede apuntar al binario para anteponer su
/// carpeta al PATH del proceso). Sin verificador, los scripts se
/// convierten igualmente pero sin la garantía, y el informe lo
/// anota. El directorio de trabajo es compartido: el verificador
/// solo sirve para llamadas secuenciales (editor y MCP lo son).
pub fn crear_verificador() -> Option<Box<VerificadorCompilacion>> {
    let fachada = buscar_fachada()?;
    if let Ok(dotnet) = std::env::var("GLASS_DOTNET") {
        if let Some(padre) = PathBuf::from(&dotnet).parent() {
            let previo = std::env::var("PATH").unwrap_or_default();
            std::env::set_var("PATH", format!("{}:{previo}", padre.display()));
        }
    }
    let sonda = std::process::Command::new("dotnet")
        .arg("--version")
        .output();
    match sonda {
        Ok(salida) if salida.status.success() => {}
        _ => return None,
    }
    let dir = std::env::temp_dir().join("glass_ripper_verificacion");
    Some(Box::new(move |codigo: &str| {
        verificar_compilacion(codigo, "ScriptImportado.cs", &fachada, &dir)
    }))
}

/// Resumen de una línea del informe de importación, para la barra
/// de estado del editor y la respuesta corta del MCP.
pub fn resumen_importacion(informe: &InformeImportacion) -> String {
    let contar = |buscado: EstadoInforme| {
        informe
            .lineas
            .iter()
            .filter(|l| l.estado == buscado)
            .count()
    };
    format!(
        "{} convertidos · {} copiados · {} con avisos · {} no convertidos · {} rechazados · {} omitidos por colisión",
        contar(EstadoInforme::Convertido),
        contar(EstadoInforme::Copiado),
        contar(EstadoInforme::ConvertidoConAvisos),
        contar(EstadoInforme::NoConvertido),
        contar(EstadoInforme::Rechazado),
        contar(EstadoInforme::OmitidoPorColision),
    )
}
