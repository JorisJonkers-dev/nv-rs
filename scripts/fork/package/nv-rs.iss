; Inno Setup script for the nv-rs Windows installer (docs/fork/WINDOWS.md says why Inno Setup and not an MSI).
; Built by build-installer.ps1:  ISCC /DVersion=... /DVersionNumeric=a.b.c.d /DStage=<folder> /DOutputDir=<folder> nv-rs.iss
; The installer is unsigned. It puts the portable package's files, unchanged, in a folder the player chooses.
#ifndef Version
  #error Pass /DVersion=<text>
#endif
#ifndef VersionNumeric
  #define VersionNumeric "0.0.0.0"
#endif
#ifndef Stage
  #error Pass /DStage=<the staged package folder>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

[Setup]
AppId={{508DA437-3098-4264-B99F-00EF86593912}
AppName=nv-rs
AppVersion={#Version}
AppVerName=nv-rs {#Version}
AppPublisher=JorisJonkers-dev
AppPublisherURL=https://github.com/JorisJonkers-dev/nv-rs
AppSupportURL=https://github.com/JorisJonkers-dev/nv-rs
VersionInfoVersion={#VersionNumeric}
; Per user, no administrator rights: nv-rs writes its saves and settings in its own folder.
PrivilegesRequired=lowest
DefaultDirName=C:\Games\nv-rs
DisableProgramGroupPage=yes
DisableReadyPage=no
OutputDir={#OutputDir}
OutputBaseFilename=nv-rs-setup-{#Version}-windows-x64
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
UninstallDisplayIcon={app}\Play.exe
LicenseFile={#Stage}\LICENSE

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{autoprograms}\nv-rs\Play"; Filename: "{app}\Play.exe"; WorkingDir: "{app}"
Name: "{autoprograms}\nv-rs\Play with mods"; Filename: "{app}\Play with mods.cmd"; WorkingDir: "{app}"
Name: "{autoprograms}\nv-rs\Install mods"; Filename: "{app}\Install mods.cmd"; WorkingDir: "{app}"
Name: "{autoprograms}\nv-rs\Choose game folder"; Filename: "{app}\Choose game folder.cmd"; WorkingDir: "{app}"
Name: "{autoprograms}\nv-rs\READ ME"; Filename: "{app}\READ ME.txt"
Name: "{autodesktop}\nv-rs"; Filename: "{app}\Play.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "A shortcut on the desktop"; Flags: unchecked

[Run]
Filename: "{app}\READ ME.txt"; Description: "Read the guide"; Flags: postinstall shellexec skipifsilent unchecked

[Code]
// nv-rs refuses to run inside Documents or AppData and cannot write in Program Files, so the installer refuses them too,
// in the wizard and for /DIR on a silent install.
function IsUnder(Lower, Folder: String): Boolean;
begin
  Result := (Folder <> '') and (Pos(AddBackslash(Lowercase(Folder)), Lower) = 1);
end;

function BadFolder(Dir: String): String;
var
  Lower: String;
begin
  Result := '';
  Lower := AddBackslash(Lowercase(Dir));
  // {commonpf} is the user's Programs folder in a non-administrator install, so the real folders come from the environment.
  if (IsUnder(Lower, GetEnv('ProgramFiles')) or IsUnder(Lower, GetEnv('ProgramFiles(x86)')) or
      IsUnder(Lower, GetEnv('ProgramW6432'))) then
    Result := 'Program Files'
  else if Pos(AddBackslash(Lowercase(ExpandConstant('{userdocs}'))), Lower) = 1 then
    Result := 'Documents'
  else if (Pos(AddBackslash(Lowercase(ExpandConstant('{localappdata}'))), Lower) = 1) or
          (Pos(AddBackslash(Lowercase(ExpandConstant('{userappdata}'))), Lower) = 1) then
    Result := 'AppData';
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Bad: String;
begin
  Result := True;
  if (CurPageID = wpSelectDir) and not WizardSilent then
  begin
    Bad := BadFolder(WizardDirValue);
    if Bad <> '' then
    begin
      MsgBox('nv-rs keeps your saves and settings in its own folder and keeps away from ' + Bad +
        '. Choose another folder, for example C:\Games\nv-rs.', mbError, MB_OK);
      Result := False;
    end;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Bad, Text: String;
begin
  Result := '';
  Bad := BadFolder(ExpandConstant('{app}'));
  if Bad <> '' then
  begin
    Text := 'nv-rs cannot be installed in ' + Bad + ' (it keeps its saves and settings beside its programs). Use another folder, for example C:\Games\nv-rs.';
    if WizardSilent then
    begin
      // No message box on a silent install: log the reason and stop before anything is written.
      Log(Text);
      Abort;
    end;
    Result := Text;
  end;
end;
