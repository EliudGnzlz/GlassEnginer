//! Driver de importación de un `.unitypackage` desde la terminal.
//!
//! Uso: `importar <paquete.unitypackage> <raiz_assets> [ruta_informe]`
//!
//! Importa sin verificador de compilación (como la corrida de
//! verificación de NeoFPS) y vuelca el informe a la salida estándar y,
//! si se pide, a un archivo. Sirve para importar proyectos reales
//! sin pasar por el editor.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("uso: importar <paquete.unitypackage> <raiz_assets> [ruta_informe]");
        return ExitCode::from(2);
    }
    let paquete = std::path::Path::new(&args[1]);
    let raiz = std::path::Path::new(&args[2]);
    match glass_ripper::importador::importar_paquete(paquete, raiz, false, None) {
        Ok(informe) => {
            let texto = informe.a_texto();
            println!("{texto}");
            if let Some(ruta) = args.get(3) {
                if let Err(e) = std::fs::write(ruta, &texto) {
                    eprintln!("no se pudo escribir el informe en {ruta}: {e}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("la importación falló: {e}");
            ExitCode::FAILURE
        }
    }
}
