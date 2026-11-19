//! Lanzamiento de procesos hijos sin ventana de consola en Windows.

/// Prepara `comando` para que el hijo NO abra una ventana de consola
/// propia al lanzarlo desde la aplicación gráfica (en Windows, un hijo
/// de consola como Blender o `dotnet` abriría una CMD visible).
/// En el resto de plataformas no hace nada. Devuelve el mismo comando
/// para poder encadenar la llamada.
pub fn sin_consola(comando: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: el hijo corre sin consola propia; sus
        // stdout/stderr heredados siguen funcionando igual.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        comando.creation_flags(CREATE_NO_WINDOW);
    }
    comando
}
