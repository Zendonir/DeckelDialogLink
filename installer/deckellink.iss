; Inno Setup script for DeckelLink (Windows 10/11, x64).
; Build:  iscc /DAppVersion=0.4.0 installer\deckellink.iss
; Input:  dist\DeckelLink\  (PyInstaller onedir build, see deckellink.spec)
; Output: dist\installer\DeckelLink-<version>-Setup-x64.exe
;
; Installs per user without administrator rights by default
; (%LOCALAPPDATA%\Programs\DeckelLink). User data (settings, profiles, received
; programs, parameter backups, logs) lives in %LOCALAPPDATA%\DeckelLink and is
; never touched by setup or uninstall.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\DeckelLink"
#endif
#define AppName "DeckelLink"
#define AppExe "DeckelLink.exe"

[Setup]
AppId={{8C3E2A47-5D1B-4E7A-9C6F-2B0D4E1A7F35}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=DeckelLink Contributors
AppPublisherURL=https://github.com/Zendonir/DeckelDialogLink
AppSupportURL=https://github.com/Zendonir/DeckelDialogLink/issues
AppComments=NC-Datenuebertragung DECKEL FP2A / DIALOG 11 (nur Datenuebertragung)
VersionInfoVersion={#AppVersion}
VersionInfoProductVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist\installer
OutputBaseFilename={#AppName}-{#AppVersion}-Setup-x64
SetupIconFile=..\src\deckellink\resources\icons\deckellink.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName} {#AppVersion}
LicenseFile=..\LICENSE
InfoBeforeFile=info_before_de.txt
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assocnc"; Description: "NC-Dateien (.nc) im Kontextmenue 'Oeffnen mit' anbieten"; GroupDescription: "Dateizuordnung:"; Flags: unchecked

[Registry]
; Only offered under "Open with" - existing default programs for .nc are not replaced.
Root: HKA; Subkey: "Software\Classes\.nc\OpenWithProgids"; ValueType: string; ValueName: "DeckelLink.NCFile"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLink.NCFile"; ValueType: string; ValueName: ""; ValueData: "NC-Programm (DeckelLink)"; Flags: uninsdeletekey; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLink.NCFile\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLink.NCFile\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assocnc

[InstallDelete]
; Remove libraries of a previous version before installing the new onedir build.
Type: filesandordirs; Name: "{app}\_internal"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "NC-Programme uebertragen"
Name: "{autoprograms}\{#AppName} Handbuch"; Filename: "{app}\README.md"; Comment: "Bedienungsanleitung"
Name: "{autoprograms}\{#AppName} (Simulationsmodus)"; Filename: "{app}\{#AppExe}"; Parameters: "--simulate"; Comment: "DeckelLink im Simulationsmodus starten"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
