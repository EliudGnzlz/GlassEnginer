; Instalador de GlassEnginer (Inno Setup 6).
; Parámetros por /D desde la línea de comandos de ISCC, con valores por defecto.

#ifndef AppVersion
  #define AppVersion "0.1.1-alpha"
#endif
#ifndef PayloadDir
  #define PayloadDir "..\dist\hub"
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
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UninstallDisplayIcon={app}\GlassHub.exe
#if FileExists(IconFile)
SetupIconFile={#IconFile}
#endif

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#PayloadDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
#ifdef EngineSeedDir
Source: "{#EngineSeedDir}\*"; DestDir: "{code:GetDataRoot}\engines\{#AppVersion}"; Flags: ignoreversion recursesubdirs createallsubdirs uninsneveruninstall
#endif

[Icons]
Name: "{group}\GlassEnginer"; Filename: "{app}\GlassHub.exe"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,GlassEnginer}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\GlassEnginer"; Filename: "{app}\GlassHub.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\GlassHub.exe"; WorkingDir: "{app}"; Description: "{cm:LaunchProgram,GlassEnginer}"; Flags: nowait postinstall skipifsilent

[Code]
function GetDataRoot(Param: String): String;
var
  UserProfile: String;
begin
  UserProfile := GetEnv('USERPROFILE');
  if UserProfile <> '' then
    Result := UserProfile + '\GlassEnginer'
  else
    Result := ExpandConstant('{localappdata}') + '\GlassEnginer';
end;

#ifdef EngineSeedDir
procedure CurStepChanged(CurStep: TSetupStep);
var
  EngineDir: String;
  EnginesJson: String;
  Json: String;
begin
  if CurStep = ssPostInstall then
  begin
    EngineDir := GetDataRoot('') + '\engines\{#AppVersion}';
    EnginesJson := GetDataRoot('') + '\engines.json';

    if DirExists(EngineDir) and not FileExists(EnginesJson) then
    begin
      Json := Chr(123) + '"en_uso": "' + '{#AppVersion}' + '"' + Chr(125);
      if not SaveStringToFile(EnginesJson, Json, False) then
        MsgBox('No se pudo escribir el registro del motor (' + EnginesJson + '). ' +
               'El Hub está instalado, pero el motor sembrado no quedará marcado como «en uso».',
               mbError, MB_OK);
    end;
  end;
end;
#endif
