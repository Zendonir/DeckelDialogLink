; Inno Setup script for the Rust build of DeckelLink (Windows 10/11, x64).
; Build:  iscc /DAppVersion=0.8.0-alpha.8 /DNumericVersion=0.8.0 installer\deckellink-rust.iss
; Input:  rust\target\release\deckellink.exe and deckellink-cli.exe (cargo build --release)
; Output: dist\installer\DeckelLink-Rust-<version>-Setup-x64.exe
;
; Separate product (own AppId and program folder) so that it can be installed next to the
; Python version during the transition. Installs per user without administrator rights by
; default (%LOCALAPPDATA%\Programs\DeckelLink-Rust). User data (settings, profiles, received
; programs, program management, parameter backups, logs) lives in %LOCALAPPDATA%\DeckelLink,
; is shared with the Python version and is never touched by setup or uninstall.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\rust\target\release"
#endif
#define AppName "DeckelLink (Rust)"
#define AppExe "deckellink.exe"

[Setup]
AppId={{4F1B9C62-7A3D-4E58-B1C0-6D2E8F9A3B71}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=DeckelLink Contributors
AppPublisherURL=https://github.com/Zendonir/DeckelDialogLink
AppSupportURL=https://github.com/Zendonir/DeckelDialogLink/issues
AppComments=NC-Datenuebertragung DECKEL FP2A / DIALOG 11 (nur Datenuebertragung), Rust-Version
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
DefaultDirName={autopf}\DeckelLink-Rust
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist\installer
OutputBaseFilename=DeckelLink-Rust-{#AppVersion}-Setup-x64
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
Root: HKA; Subkey: "Software\Classes\.nc\OpenWithProgids"; ValueType: string; ValueName: "DeckelLinkRust.NCFile"; ValueData: ""; Flags: uninsdeletevalue; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLinkRust.NCFile"; ValueType: string; ValueName: ""; ValueData: "NC-Programm (DeckelLink Rust)"; Flags: uninsdeletekey; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLinkRust.NCFile\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: assocnc
Root: HKA; Subkey: "Software\Classes\DeckelLinkRust.NCFile\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: assocnc

[Files]
Source: "{#SourceDir}\deckellink.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\deckellink-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\rust\README.md"; DestDir: "{app}"; DestName: "README-Rust.md"; Flags: ignoreversion
Source: "..\docs\*.md"; DestDir: "{app}\docs"; Flags: ignoreversion
Source: "..\rust\crates\deckellink-gui\assets\fonts\LICENSE-*.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "NC-Programme uebertragen"
Name: "{autoprograms}\{#AppName} Handbuch"; Filename: "{app}\README.md"; Comment: "Bedienungsanleitung"
Name: "{autoprograms}\{#AppName} (Simulationsmodus)"; Filename: "{app}\{#AppExe}"; Parameters: "--simulate"; Comment: "DeckelLink im Simulationsmodus starten"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
