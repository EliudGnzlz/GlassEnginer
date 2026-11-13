; Instalador de GlassEnginer (Inno Setup 6).
; Parámetros por /D desde la línea de comandos de ISCC, con valores por defecto.

#ifndef AppVersion
  #define AppVersion "0.1.1-alpha"
#endif
#ifndef PayloadDir
  #define PayloadDir "..\dist\payload"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif
#ifndef IconFile
  #define IconFile AddBackslash(SourcePath) + "glassenginer.ico"
#endif

[Setup]
AppId={{8F3C2A71-5B4E-4D2A-9C61-3E7F0A1B2C4D}
AppName=GlassEnginer
AppVersion={#AppVersion}
AppPublisher=GlassEnginer
DefaultDirName={autopf}\GlassEnginer
DefaultGroupName=GlassEnginer
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=GlassEnginer-Launcher-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
; Instalación por usuario por defecto; el diálogo permite elegir instalar para todos (admin).
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UninstallDisplayIcon={app}\glass_app.exe
#if FileExists(IconFile)
SetupIconFile={#IconFile}
#endif

; Solo el idioma por defecto: Spanish.isl no está garantizado en todos los runners
; y su ausencia haría fallar la compilación.
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Todo el payload: binario, managed\... y assets\...
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

; WorkingDir={app} es imprescindible: la app resuelve managed\ y assets\ desde el directorio de trabajo.
[Icons]
Name: "{group}\GlassEnginer"; Filename: "{app}\glass_app.exe"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,GlassEnginer}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\GlassEnginer"; Filename: "{app}\glass_app.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\glass_app.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,GlassEnginer}"; Flags: nowait postinstall skipifsilent
