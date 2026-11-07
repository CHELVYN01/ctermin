; Inno Setup script for ctermin. Build the release binary first, then run from the repo root:
;   cargo build --release
;   iscc installer\ctermin.iss
; The installer is written to target\installer.

#define AppName "ctermin"
#define AppVersion "0.1.0"
#define AppExe "ctermin.exe"

[Setup]
AppId={{7D2E9A41-5C3B-4F6E-9A1D-C7E2B8F40A16}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=CHELVYN01
AppPublisherURL=https://github.com/CHELVYN01/ctermin
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Installs for the current user without administrator rights.
PrivilegesRequired=lowest
LicenseFile=..\LICENSE
OutputDir=..\target\installer
OutputBaseFilename=ctermin-{#AppVersion}-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; ConPTY needs Windows 10 version 1809.
MinVersion=10.0.17763

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

; Settings and servers in %APPDATA%\ctermin, and passwords in Credential Manager, are kept on
; uninstall so a reinstall keeps working.
