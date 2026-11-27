; Inno Setup script for the Windows installer (built by CI; locally:
; iscc /DVersion=0.4.1 packaging\windows\coverant.iss).
; Installs for the current user by default (no administrator rights needed), or for all users
; if chosen; Start menu entry, optional desktop shortcut, and an entry in Settings -> Apps to
; uninstall it. Uninstalling leaves the user's sites and the terrain cache in place.

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\coverant.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{55B43945-10A5-49E3-A139-6E26FAF2F683}
AppName=Coverant
AppVersion={#Version}
AppVerName=Coverant {#Version}
AppPublisher=Coverant
AppPublisherURL=https://github.com/tanganyika/coverant
AppSupportURL=https://github.com/tanganyika/coverant/issues
AppUpdatesURL=https://github.com/tanganyika/coverant/releases
DefaultDirName={autopf}\Coverant
DefaultGroupName=Coverant
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\..\LICENSE
SetupIconFile=..\coverant.ico
UninstallDisplayIcon={app}\coverant.exe
UninstallDisplayName=Coverant
OutputDir={#OutputDir}
OutputBaseFilename=coverant-{#Version}-windows-x86_64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\CHANGELOG.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Coverant"; Filename: "{app}\coverant.exe"
Name: "{autodesktop}\Coverant"; Filename: "{app}\coverant.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\coverant.exe"; Description: "{cm:LaunchProgram,Coverant}"; Flags: nowait postinstall skipifsilent
