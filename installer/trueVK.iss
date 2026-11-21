[Setup]
AppName=trueVK
AppVersion={#AppVersion}
AppId={{BB379A13-E10B-42A6-A1B6-EE1CDE12401A}}
DefaultDirName={localappdata}\Programs\trueVK
DefaultGroupName=trueVK
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
OutputDir=Output
OutputBaseFilename=trueVK-Setup-{#AppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\truevk.exe
CloseApplications=yes

[Languages]
Name: en; MessagesFile: "compiler:Default.isl"
Name: ru; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: desktopicon; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\truevk.exe"; DestDir: "{app}"; DestName: "truevk.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\trueVK"; Filename: "{app}\truevk.exe"
Name: "{autodesktop}\trueVK"; Filename: "{app}\truevk.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\truevk.exe"; Description: "Launch trueVK"; Flags: postinstall nowait skipifsilent
