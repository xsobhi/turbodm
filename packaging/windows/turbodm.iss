; TurboDM installer (Inno Setup 6). Built by .github/workflows/release.yml:
;   iscc /DVersion=1.4.0 /DArch=x64 packaging\windows\turbodm.iss
; Installs for the current user (no admin rights needed), like Chrome or VS Code's user setup.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif

[Setup]
AppId={{6B0C7A52-3E0B-4B7E-9C1D-7A4D2F1B9E21}
AppName=TurboDM
AppVersion={#Version}
AppPublisher=xsobhi
AppPublisherURL=https://github.com/xsobhi/turbodm
AppSupportURL=https://github.com/xsobhi/turbodm/issues
DefaultDirName={autopf}\TurboDM
DefaultGroupName=TurboDM
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=..\..\dist\windows
OutputBaseFilename=TurboDM-{#Version}-windows-{#Arch}-setup
SetupIconFile=..\..\data\turbodm.ico
UninstallDisplayIcon={app}\bin\turbodm.exe
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
#if Arch == "x64"
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\..\dist\windows\TurboDM\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{group}\TurboDM"; Filename: "{app}\bin\turbodm.exe"
Name: "{group}\Browser extension folder"; Filename: "{app}\extension"
Name: "{group}\Uninstall TurboDM"; Filename: "{uninstallexe}"
Name: "{autodesktop}\TurboDM"; Filename: "{app}\bin\turbodm.exe"; Tasks: desktopicon

[Run]
; connect the browser extension (native messaging host in the registry)
Filename: "{app}\bin\turbodm.exe"; Parameters: "--register"; Flags: runhidden
Filename: "{app}\bin\turbodm.exe"; Description: "{cm:LaunchProgram,TurboDM}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{app}\bin\turbodm.exe"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "Unregister"
