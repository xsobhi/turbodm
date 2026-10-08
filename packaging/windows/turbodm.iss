; TurboDM installer (Inno Setup 6.6+). Built by .github/workflows/release.yml:
;   iscc /DVersion=1.4.1 /DArch=x64 packaging\windows\turbodm.iss
; Like most apps: one admin prompt, installed for everyone in Program Files, and only the
; folder to choose (first install). Light or dark, following Windows.

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
DisableWelcomePage=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableDirPage=auto
ShowLanguageDialog=no
WizardStyle=modern dynamic
WizardImageFile={#Art("wizard")}
WizardSmallImageFile={#Art("wizard-small")}
WizardImageAlphaFormat=defined
SetupIconFile=..\..\data\turbodm.ico
UninstallDisplayIcon={app}\bin\turbodm.exe
UninstallDisplayName=TurboDM
OutputDir=..\..\dist\windows
OutputBaseFilename=TurboDM-{#Version}-windows-{#Arch}-setup
Compression=lzma2/max
SolidCompression=yes
; versions with the GTK runtime left gdbus.exe running from {app}: close it rather than abort
CloseApplications=force
#if Arch == "x64"
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif

[Messages]
WelcomeLabel1=Welcome to TurboDM
WelcomeLabel2=TurboDM {#Version} will be installed on this computer.%n%nDownload files over many connections at once, pause and resume them any time, and catch downloads straight from your browser.
FinishedHeadingLabel=TurboDM is ready
FinishedLabel=TurboDM is installed. When it opens, it helps you add its extension to your browser, to catch your downloads.
ClickFinish=

[InstallDelete]
; TurboDM before 1.6 shipped the GTK runtime: now it's one turbodm.exe using Windows' own controls
Type: filesandordirs; Name: "{app}\lib"
Type: filesandordirs; Name: "{app}\share"
Type: files; Name: "{app}\bin\*.dll"
Type: files; Name: "{app}\bin\gdbus.exe"

[Files]
Source: "..\..\dist\windows\TurboDM\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\TurboDM"; Filename: "{app}\bin\turbodm.exe"
Name: "{autodesktop}\TurboDM"; Filename: "{app}\bin\turbodm.exe"

[Run]
; connect the browser extension for every user of this PC
Filename: "{app}\bin\turbodm.exe"; Parameters: "--register-system"; Flags: runhidden
Filename: "{app}\bin\turbodm.exe"; Description: "Open TurboDM"; Flags: nowait postinstall skipifsilent runasoriginaluser
; updating from inside TurboDM runs this installer with /SILENT: open the new version afterwards
Filename: "{app}\bin\turbodm.exe"; Flags: nowait runasoriginaluser; Check: WizardSilent

[UninstallRun]
Filename: "{app}\bin\turbodm.exe"; Parameters: "--unregister-system"; Flags: runhidden; RunOnceId: "Unregister"

[UninstallDelete]
Type: filesandordirs; Name: "{app}\native-messaging"

[Code]
// Before installing over a copy: end everything still running from its folder (TurboDM, and
// the gdbus.exe helper its GTK runtime starts, which outlives it), then remove a per-user
// TurboDM 1.4.0 from AppData so only this copy is left.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Uninstaller: String;
  Code: Integer;
begin
  Result := '';
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
       '-NoProfile -NonInteractive -Command "Get-Process | Where-Object { $_.Path -like ''' +
       ExpandConstant('{app}') + '\*'' } | Stop-Process -Force"', '', SW_HIDE, ewWaitUntilTerminated, Code);
  if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{6B0C7A52-3E0B-4B7E-9C1D-7A4D2F1B9E21}_is1',
                         'UninstallString', Uninstaller) then
    Exec(RemoveQuotes(Uninstaller), '/VERYSILENT /SUPPRESSMSGBOXES /NORESTART', '', SW_HIDE,
         ewWaitUntilTerminated, Code);
end;
