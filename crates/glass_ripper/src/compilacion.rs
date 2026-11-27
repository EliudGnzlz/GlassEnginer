//! Garantía de compilación real de los scripts convertidos contra la fachada.
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Compila un script contra la fachada real mediante un proyecto temporal.
pub fn verificar_compilacion(
    codigo: &str,
    nombre_archivo: &str,
    proyecto_core: &Path,
    dir_trabajo: &Path,
) -> Result<(), Vec<String>> {
    if let Err(error) = preparar_directorio(dir_trabajo) {
        return Err(vec![format!(
            "no se pudo preparar el directorio de compilación: {error}"
        )]);
    }

    let proyecto_core = match fs::canonicalize(proyecto_core) {
        Ok(ruta) => ruta,
        Err(error) => {
            return Err(vec![format!(
                "no se pudo resolver el proyecto de la fachada: {error}"
            )]);
        }
    };

    let nombre_archivo = nombre_archivo
        .rsplit(['/', '\\'])
        .find(|parte| !parte.is_empty())
        .unwrap_or("Script.cs");
    let ruta_script = dir_trabajo.join(nombre_archivo);

    if let Err(error) = fs::write(&ruta_script, codigo) {
        return Err(vec![format!("no se pudo escribir el script: {error}")]);
    }

    let proyecto = format!(
        "<Project Sdk=\"Microsoft.NET.Sdk\">\n\
         \x20 <PropertyGroup>\n\
         \x20   <TargetFramework>net8.0</TargetFramework>\n\
         \x20   <ImplicitUsings>enable</ImplicitUsings>\n\
         \x20   <Nullable>enable</Nullable>\n\
         \x20   <EnableDefaultCompileItems>false</EnableDefaultCompileItems>\n\
         \x20 </PropertyGroup>\n\
         \x20 <ItemGroup>\n\
         \x20   <Compile Include=\"{}\" />\n\
         \x20   <ProjectReference Include=\"{}\" />\n\
         \x20 </ItemGroup>\n\
         </Project>\n",
        escapar_xml(nombre_archivo),
        escapar_xml(&proyecto_core.to_string_lossy())
    );

    if let Err(error) = fs::write(dir_trabajo.join("Prueba.csproj"), proyecto) {
        return Err(vec![format!("no se pudo escribir el proyecto araña: {error}")]);
    }

    let salida = match Command::new("dotnet")
        .args(["build", "--nologo", "-v", "q"])
        .current_dir(dir_trabajo)
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .output()
    {
        Ok(salida) => salida,
        Err(error) => return Err(vec![format!("no se pudo ejecutar dotnet: {error}")]),
    };

    if salida.status.success() {
        return Ok(());
    }

    let texto = format!(
        "{}\n{}",
        String::from_utf8_lossy(&salida.stdout),
        String::from_utf8_lossy(&salida.stderr)
    );
    let errores = extraer_errores(&texto);
    if errores.is_empty() {
        // Ajuste al integrar (Rayo): sin errores CS, la causa real
        // (p. ej. «No .NET SDKs were found») va en el mensaje, no se
        // esconde tras un texto genérico.
        let cola: Vec<&str> = texto
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let detalle = cola[cola.len().saturating_sub(3)..].join(" | ");
        Err(vec![format!(
            "dotnet build falló sin líneas «error CS» en la salida: {detalle}"
        )])
    } else {
        Err(errores)
    }
}

/// Vacía el directorio de trabajo sin seguir enlaces simbólicos a directorios.
fn preparar_directorio(dir_trabajo: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir_trabajo)?;
    for entrada in fs::read_dir(dir_trabajo)? {
        let entrada = entrada?;
        if entrada.file_type()?.is_dir() {
            fs::remove_dir_all(entrada.path())?;
        } else {
            fs::remove_file(entrada.path())?;
        }
    }
    Ok(())
}

/// Extrae errores del compilador conservando su orden, sin duplicados.
fn extraer_errores(salida: &str) -> Vec<String> {
    let mut vistas = HashSet::new();
    let mut errores = Vec::new();

    for linea in salida.lines().map(str::trim) {
        if linea.contains("error CS") && vistas.insert(linea.to_string()) {
            errores.push(linea.to_string());
            if errores.len() == 10 {
                break;
            }
        }
    }

    errores
}

fn escapar_xml(texto: &str) -> String {
    let mut escapado = String::with_capacity(texto.len());
    for caracter in texto.chars() {
        match caracter {
            '&' => escapado.push_str("&amp;"),
            '<' => escapado.push_str("&lt;"),
            '>' => escapado.push_str("&gt;"),
            '"' => escapado.push_str("&quot;"),
            '\'' => escapado.push_str("&apos;"),
            _ => escapado.push(caracter),
        }
    }
    escapado
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn extrae_errores_sin_duplicados_y_con_tope() {
        let mut salida = String::from("aviso previo\n");
        salida.push_str("Foo.cs(1,1): error CS0001: duplicado\n");
        salida.push_str("Foo.cs(1,1): error CS0001: duplicado\n");
        for numero in 2..=12 {
            salida.push_str(&format!(
                "Foo.cs({numero},1): error CS{numero:04}: error distinto\n"
            ));
        }

        let errores = extraer_errores(&salida);

        assert_eq!(errores.len(), 10);
        assert_eq!(errores[0], "Foo.cs(1,1): error CS0001: duplicado");
        assert_eq!(
            errores.iter().filter(|error| *error == &errores[0]).count(),
            1
        );
        assert!(errores[9].contains("error CS0010"));
    }

    #[test]
    fn compila_con_dotnet_real_si_se_solicita() {
        let Some(proyecto_core) = std::env::var_os("GLASS_RIPPER_PRUEBA_DOTNET") else {
            return;
        };
        let proyecto_core = PathBuf::from(proyecto_core);
        let base = directorio_temporal();

        let valido = "using UnityEngine;\npublic class PruebaDirecta : MonoBehaviour { }\n";
        let resultado = verificar_compilacion(
            valido,
            "Assets/PruebaDirecta.cs",
            &proyecto_core,
            &base.join("valido"),
        );
        assert!(resultado.is_ok(), "script directo rechazado: {resultado:?}");

        let corrutina = "using UnityEngine;\nusing System.Collections;\n\
                        public class PruebaCorrutina : MonoBehaviour {\n\
                        public void Iniciar() { StartCoroutine(Rutina()); }\n\
                        private IEnumerator Rutina() { yield break; }\n\
                        }\n";
        let resultado = verificar_compilacion(
            corrutina,
            "PruebaCorrutina.cs",
            &proyecto_core,
            &base.join("corrutina"),
        );
        assert!(resultado.is_ok(), "{:?}", resultado);

        let _ = fs::remove_dir_all(base);

    }

    fn directorio_temporal() -> PathBuf {
        let instante = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("el reloj del sistema debe ser posterior a UNIX_EPOCH")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "glass_ripper_compilacion_{}_{}",
            std::process::id(),
            instante
        ))
    }
}
