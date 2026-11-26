//! Compilación y carga de los scripts C# del proyecto abierto.
//!
//! El motor solo conoce las clases de la fachada (GlassEngine.Core.dll). Este
//! módulo compila los .cs bajo los Assets del proyecto contra la fachada y
//! devuelve la ruta del DLL resultante para que el host lo cargue.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime};

const NOMBRE_ENSAMBLADO: &str = "GlassProjectScripts";
const DIRECTORIOS_EXCLUIDOS: [&str; 3] = ["Editor", "bin", "obj"];
const MAX_LINEAS_ERROR: usize = 15;

/// Compila (si hace falta) los scripts del proyecto y devuelve la ruta del
/// DLL. None si el proyecto no tiene scripts o si la compilación falla
/// (en ese caso ya se ha logueado el motivo).
pub fn asegurar_ensamblado_scripts(raiz_proyecto: &Path, raiz_motor: &Path) -> Option<PathBuf> {
    let raiz_proyecto = absoluta(raiz_proyecto);
    let raiz_motor = absoluta(raiz_motor);

    let raices = raices_assets(&raiz_proyecto);
    let scripts = recolectar_scripts(&raices);
    if scripts.is_empty() {
        return None;
    }

    let dir_build = raiz_proyecto.join(".glass").join("scripts_build");
    let dll = dir_build.join("bin").join(format!("{NOMBRE_ENSAMBLADO}.dll"));
    let fachada_dll = raiz_motor
        .join("managed")
        .join("GlassEngine.Core")
        .join("bin")
        .join("Debug")
        .join("net8.0")
        .join("GlassEngine.Core.dll");

    if esta_fresco(&dll, &scripts, &fachada_dll) {
        return Some(dll);
    }

    let fachada_csproj = raiz_motor
        .join("managed")
        .join("GlassEngine.Core")
        .join("GlassEngine.Core.csproj");
    if !fachada_csproj.is_file() {
        log::error!(
            "No se encuentra el proyecto de la fachada en {}: no se pueden compilar los scripts del proyecto.",
            fachada_csproj.display()
        );
        return None;
    }

    if let Err(e) = fs::create_dir_all(&dir_build) {
        log::error!(
            "No se pudo crear el directorio de construcción de scripts {}: {e}",
            dir_build.display()
        );
        return None;
    }

    let csproj = dir_build.join(format!("{NOMBRE_ENSAMBLADO}.csproj"));
    if let Err(e) = fs::write(&csproj, texto_csproj(&raices, &fachada_csproj)) {
        log::error!("No se pudo escribir {}: {e}", csproj.display());
        return None;
    }

    let dotnet = std::env::var_os("GLASS_DOTNET")
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "dotnet".into());

    let inicio = Instant::now();
    let salida = match Command::new(&dotnet)
        .arg("build")
        .arg("--nologo")
        .arg("-v")
        .arg("q")
        .arg(&csproj)
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
        .current_dir(&dir_build)
        .output()
    {
        Ok(salida) => salida,
        Err(e) => {
            log::error!(
                "No se pudo ejecutar {:?} para compilar los scripts del proyecto: {e}",
                dotnet
            );
            return None;
        }
    };
    let duracion = inicio.elapsed();

    if salida.status.success() && dll.is_file() {
        log::info!(
            "Scripts del proyecto compilados: {} archivos .cs en {:.1} s -> {}",
            scripts.len(),
            duracion.as_secs_f64(),
            dll.display()
        );
        return Some(dll);
    }

    let mut texto = String::from_utf8_lossy(&salida.stdout).into_owned();
    texto.push('\n');
    texto.push_str(&String::from_utf8_lossy(&salida.stderr));

    let mut errores = lineas_unicas(&texto, "error CS");
    if errores.is_empty() {
        // Sin errores de C#: puede ser un error de MSBuild o del SDK (MSBxxxx, NETSDKxxxx).
        errores = lineas_unicas(&texto, ": error ");
    }

    if errores.is_empty() {
        let ultimas: Vec<&str> = texto
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let desde = ultimas.len().saturating_sub(MAX_LINEAS_ERROR);
        log::error!(
            "La compilación de los scripts del proyecto falló (código {:?}, DLL generado: {}) sin errores reconocibles. Salida:\n{}",
            salida.status.code(),
            dll.is_file(),
            ultimas[desde..].join("\n")
        );
        return None;
    }

    let total = errores.len();
    let mut detalle = errores
        .iter()
        .take(MAX_LINEAS_ERROR)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if total > MAX_LINEAS_ERROR {
        detalle.push_str(&format!("\n... y {} más", total - MAX_LINEAS_ERROR));
    }
    log::error!(
        "La compilación de los scripts del proyecto falló con {total} errores únicos:\n{detalle}"
    );
    None
}

/// Contenido del .csproj que compila los scripts del proyecto contra la fachada.
fn texto_csproj(raices_assets: &[PathBuf], fachada_csproj: &Path) -> String {
    let mut compile = String::new();
    for raiz in raices_assets {
        let r = escapar_xml(&escapar_msbuild(&ruta_msbuild(raiz)));
        compile.push_str(&format!(
            "    <Compile Include=\"{r}/**/*.cs\" Exclude=\"{r}/**/Editor/**/*.cs;{r}/**/bin/**/*.cs;{r}/**/obj/**/*.cs\" />\n"
        ));
    }
    let fachada = escapar_xml(&escapar_msbuild(&ruta_msbuild(fachada_csproj)));

    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <!-- Generado por GlassEngine: no editar a mano, se reescribe en cada compilación. -->
  <PropertyGroup>
    <TargetFramework>net8.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <EnableDefaultCompileItems>false</EnableDefaultCompileItems>
    <AssemblyName>{NOMBRE_ENSAMBLADO}</AssemblyName>
    <OutputPath>bin\</OutputPath>
    <!-- Sin esto el SDK añade net8.0\ a OutputPath y el DLL no queda en bin\. -->
    <AppendTargetFrameworkToOutputPath>false</AppendTargetFrameworkToOutputPath>
    <AppendRuntimeIdentifierToOutputPath>false</AppendRuntimeIdentifierToOutputPath>
  </PropertyGroup>
  <ItemGroup>
{compile}  </ItemGroup>
  <ItemGroup>
    <!-- Private=false: la fachada ya está cargada en el runtime; una copia junto
         al DLL podría cargarse dos veces y duplicar los tipos (GlassBehaviour). -->
    <ProjectReference Include="{fachada}" Private="false" />
  </ItemGroup>
</Project>
"#
    )
}

/// true si el DLL existe y no es más antiguo que ningún script ni que la
/// fachada construida (esta última solo cuenta si existe).
fn esta_fresco(dll: &Path, scripts: &[PathBuf], fachada_dll: &Path) -> bool {
    let Some(mtime_dll) = mtime(dll) else {
        return false;
    };
    let scripts_ok = scripts
        .iter()
        .all(|s| mtime(s).is_some_and(|m| mtime_dll >= m));
    if !scripts_ok {
        return false;
    }
    if fachada_dll.exists() {
        return mtime(fachada_dll).is_some_and(|m| mtime_dll >= m);
    }
    true
}

fn mtime(ruta: &Path) -> Option<SystemTime> {
    fs::metadata(ruta).and_then(|m| m.modified()).ok()
}

/// `Assets` y `assets` directos bajo la raíz del proyecto. Se listan las
/// entradas reales para no duplicar la misma carpeta en sistemas de archivos
/// que no distinguen mayúsculas.
fn raices_assets(raiz_proyecto: &Path) -> Vec<PathBuf> {
    let mut raices = Vec::new();
    let Ok(entradas) = fs::read_dir(raiz_proyecto) else {
        return raices;
    };
    for entrada in entradas.flatten() {
        let es_assets = matches!(entrada.file_name().to_str(), Some("Assets") | Some("assets"));
        let ruta = entrada.path();
        if es_assets && ruta.is_dir() {
            raices.push(ruta);
        }
    }
    raices.sort();
    raices
}

fn recolectar_scripts(raices: &[PathBuf]) -> Vec<PathBuf> {
    let mut scripts = Vec::new();
    for raiz in raices {
        recorrer(raiz, &mut scripts);
    }
    scripts
}

/// Recorrido recursivo sin seguir enlaces simbólicos a directorios (evita ciclos).
fn recorrer(dir: &Path, scripts: &mut Vec<PathBuf>) {
    let Ok(entradas) = fs::read_dir(dir) else {
        return;
    };
    for entrada in entradas.flatten() {
        let Ok(tipo) = entrada.file_type() else {
            continue;
        };
        let ruta = entrada.path();
        if tipo.is_dir() {
            let excluido = entrada
                .file_name()
                .to_str()
                .is_some_and(|n| DIRECTORIOS_EXCLUIDOS.contains(&n));
            if !excluido {
                recorrer(&ruta, scripts);
            }
        } else if ruta.extension().is_some_and(|e| e == "cs") {
            scripts.push(ruta);
        }
    }
}

/// Líneas (recortadas) que contienen `patron`, sin repetir y en orden de aparición.
fn lineas_unicas(texto: &str, patron: &str) -> Vec<String> {
    let mut vistas = HashSet::new();
    texto
        .lines()
        .map(str::trim)
        .filter(|l| l.contains(patron))
        .filter(|l| vistas.insert(l.to_string()))
        .map(str::to_string)
        .collect()
}

fn absoluta(ruta: &Path) -> PathBuf {
    if ruta.is_absolute() {
        return ruta.to_path_buf();
    }
    std::env::current_dir()
        .map(|d| d.join(ruta))
        .unwrap_or_else(|_| ruta.to_path_buf())
}

/// Ruta como texto sin separador final (los globs añaden su propio `/`).
fn ruta_msbuild(ruta: &Path) -> String {
    let texto = ruta.display().to_string();
    let recortado = texto.trim_end_matches(['/', '\\']);
    if recortado.is_empty() {
        texto
    } else {
        recortado.to_string()
    }
}

/// Caracteres con significado especial en las listas de items de MSBuild.
fn escapar_msbuild(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    for c in texto.chars() {
        match c {
            '%' => salida.push_str("%25"),
            ';' => salida.push_str("%3B"),
            '$' => salida.push_str("%24"),
            '@' => salida.push_str("%40"),
            _ => salida.push(c),
        }
    }
    salida
}

fn escapar_xml(texto: &str) -> String {
    let mut salida = String::with_capacity(texto.len());
    for c in texto.chars() {
        match c {
            '&' => salida.push_str("&amp;"),
            '<' => salida.push_str("&lt;"),
            '>' => salida.push_str("&gt;"),
            '"' => salida.push_str("&quot;"),
            _ => salida.push(c),
        }
    }
    salida
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn dir_temporal(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "glass_project_scripts_{nombre}_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn crear_con_mtime(ruta: &Path, momento: SystemTime) {
        if let Some(padre) = ruta.parent() {
            fs::create_dir_all(padre).unwrap();
        }
        let archivo = fs::File::create(ruta).unwrap();
        archivo.set_modified(momento).unwrap();
    }

    #[test]
    fn csproj_contiene_globs_exclusiones_y_fachada() {
        let raices = vec![
            PathBuf::from("/proyecto/Assets"),
            PathBuf::from("/proyecto/assets"),
        ];
        let fachada = PathBuf::from("/motor/managed/GlassEngine.Core/GlassEngine.Core.csproj");
        let texto = texto_csproj(&raices, &fachada);

        for raiz in &raices {
            let r = raiz.display().to_string();
            assert!(texto.contains(&format!("<Compile Include=\"{r}/**/*.cs\"")));
            assert!(texto.contains(&format!("{r}/**/Editor/**/*.cs")));
            assert!(texto.contains(&format!("{r}/**/bin/**/*.cs")));
            assert!(texto.contains(&format!("{r}/**/obj/**/*.cs")));
        }
        assert!(texto.contains(&format!(
            "<ProjectReference Include=\"{}\"",
            fachada.display()
        )));
        assert!(texto.contains("<EnableDefaultCompileItems>false</EnableDefaultCompileItems>"));
        assert!(texto.contains("<AssemblyName>GlassProjectScripts</AssemblyName>"));
    }

    #[test]
    fn csproj_escapa_rutas() {
        let texto = texto_csproj(
            &[PathBuf::from("/a&b/Assets")],
            Path::new("/motor/<x>/GlassEngine.Core.csproj"),
        );
        assert!(texto.contains("/a&amp;b/Assets/**/*.cs"));
        assert!(texto.contains("/motor/&lt;x&gt;/GlassEngine.Core.csproj"));
    }

    #[test]
    fn dll_mas_nuevo_que_los_scripts_esta_fresco() {
        let dir = dir_temporal("fresco");
        let base = SystemTime::now() - Duration::from_secs(1000);
        let cs = dir.join("Assets").join("Jugador.cs");
        let dll = dir.join("bin").join("GlassProjectScripts.dll");
        crear_con_mtime(&cs, base);
        crear_con_mtime(&dll, base + Duration::from_secs(10));

        assert!(esta_fresco(&dll, &[cs], &dir.join("no_existe.dll")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn script_o_fachada_mas_nuevos_que_el_dll_no_estan_frescos() {
        let dir = dir_temporal("no_fresco");
        let base = SystemTime::now() - Duration::from_secs(1000);
        let cs_viejo = dir.join("Assets").join("Viejo.cs");
        let cs_nuevo = dir.join("Assets").join("Nuevo.cs");
        let dll = dir.join("bin").join("GlassProjectScripts.dll");
        let fachada = dir.join("GlassEngine.Core.dll");
        crear_con_mtime(&cs_viejo, base);
        crear_con_mtime(&dll, base + Duration::from_secs(10));
        crear_con_mtime(&cs_nuevo, base + Duration::from_secs(20));

        let scripts = vec![cs_viejo.clone(), cs_nuevo];
        assert!(!esta_fresco(&dll, &scripts, &dir.join("no_existe.dll")));

        // Fachada reconstruida después del DLL: tampoco está fresco.
        crear_con_mtime(&fachada, base + Duration::from_secs(30));
        assert!(!esta_fresco(&dll, std::slice::from_ref(&cs_viejo), &fachada));

        // DLL inexistente: nunca fresco.
        assert!(!esta_fresco(&dir.join("falta.dll"), &[cs_viejo], &fachada));
        let _ = fs::remove_dir_all(&dir);
    }
}
