; TurboDM installer (Inno Setup 6.6+). Built by .github/workflows/release.yml:
;   iscc /DVersion=1.4.1 /DArch=x64 packaging\windows\turbodm.iss
; Like most apps: one admin prompt, installed for everyone in Program Files, no questions
; beyond the folder and a desktop shortcut. Light or dark, following Windows.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif
#define Art(Name) "art\" + Name + "-100.bmp,art\" + Name + "-125.bmp,art\" + Name + "-150.bmp,art\" + Name + "-175.bmp,art\" + Name + "-200.bmp,art\" + Name + "-250.bmp"

[Setup]
AppId={{6B0C7A52-3E0B-4B7E-9C1D-7A4D2F1B9E21}
AppName=TurboDM
AppVersion={#Version}
AppVerName=TurboDM {#Version}
AppPublisher=xsobhi
AppPublisherURL=https://github.com/xsobhi/turbodm
AppSupportURL=https://github.com/xsobhi/turbodm/issues
AppUpdatesURL=https://github.com/xsobhi/turbodm/releases
VersionInfoVersion={#Version}
VersionInfoDescription=TurboDM Setup
DefaultDirName={autopf}\TurboDM
PrivilegesRequired=admin
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableDirPage=auto
ShowLanguageDialog=no
WizardStyle=modern dynamic
WizardImageFile={#Art("wizard")}
WizardSmallImageFile={#Art("wizard-small")}
SetupIconFile=..\..\data\turbodm.ico
UninstallDisplayIcon={app}\bin\turbodm.exe
UninstallDisplayName=TurboDM
OutputDir=..\..\dist\windows
OutputBaseFilename=TurboDM-{#Version}-windows-{#Arch}-setup
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
#if Arch == "x64"
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Messages]
WelcomeLabel1=Welcome to TurboDM
WelcomeLabel2=TurboDM {#Version} will be installed on this computer.%n%nDownload files over many connections at once, pause and resume them any time, and catch downloads straight from your browser.
FinishedHeadingLabel=TurboDM is ready
FinishedLabel=TurboDM is installed. To catch downloads from your browser, add the TurboDM extension (its folder can be opened below).
ClickFinish=

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"

[Files]
Source: "..\..\dist\windows\TurboDM\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\TurboDM"; Filename: "{app}\bin\turbodm.exe"
Name: "{autodesktop}\TurboDM"; Filename: "{app}\bin\turbodm.exe"; Tasks: desktopicon

[Run]
; connect the browser extension for every user of this PC
Filename: "{app}\bin\turbodm.exe"; Parameters: "--register-system"; Flags: runhidden
Filename: "{app}\bin\turbodm.exe"; Description: "Open TurboDM"; Flags: nowait postinstall skipifsilent runasoriginaluser
Filename: "{app}\extension"; Description: "Show the browser extension folder"; Flags: postinstall shellexec skipifsilent unchecked

[UninstallRun]
Filename: "{app}\bin\turbodm.exe"; Parameters: "--unregister-system"; Flags: runhidden; RunOnceId: "Unregister"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\native-messaging"
