; Nunchi's installer (Inno Setup 6): the 64-bit and 32-bit input method in Program Files,
; registered, and put in the user's keyboard list. Built by scripts\build-installer.ps1, which
; passes the version (/DAppVersion=…).
;
; Program Files is where sandboxed apps (Store apps) can read an input method from. The DLLs an
; app has loaded can't be overwritten, so an update renames them aside first: apps already open
; keep the old version until they are restarted, apps opened after get the new one. Uninstalling
; sets them aside the same way and deletes them at restart.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define Root ".."

[Setup]
AppId={{0B8DC63D-8A1B-466F-8A28-4AADAB648E0D}
AppName={cm:AppName}
AppVersion={#AppVersion}
AppVerName={cm:AppName} {#AppVersion}
AppPublisher=mununki
AppPublisherURL=https://github.com/mununki/nunchi
AppSupportURL=https://github.com/mununki/nunchi/issues
DefaultDirName={autopf}\Nunchi
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
; x64 Windows only: on ARM64 Windows an x64 input method would work only in x64 apps, not in the
; system's own (Explorer, Edge, Notepad), so Setup says it can't install there.
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0
; In the language Windows uses (Korean or English), without asking first.
ShowLanguageDialog=auto
OutputDir={#Root}\target\installer
OutputBaseFilename=Nunchi-Setup-{#AppVersion}
SetupIconFile={#Root}\crates\nunchi-tsf\res\nunchi.ico
UninstallDisplayIcon={app}\nunchi_tsf.dll
UninstallDisplayName={cm:AppName}
VersionInfoDescription=Nunchi Setup
VersionInfoProductName=Nunchi
LicenseFile={#Root}\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Every app with a text field may have the DLL loaded: closing them all isn't Setup's business.
CloseApplications=no
RestartApplications=no

[Languages]
; After installing, a page on how to use it (who gets this installer can't see the README).
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"; InfoAfterFile: "after-install.ko.txt"
Name: "english"; MessagesFile: "compiler:Default.isl"; InfoAfterFile: "after-install.en.txt"

[CustomMessages]
; 눈치 on screen in Korean, Nunchi in English; files and folders are Nunchi.
korean.AppName=눈치
english.AppName=Nunchi
korean.SettingsShortcut=눈치 설정
english.SettingsShortcut=Nunchi Settings
korean.DefaultIme=눈치를 기본 입력기로 지정
english.DefaultIme=Make Nunchi the default input method
korean.RestartApps=이미 열려 있는 앱은 다시 시작해야 새로 설치한 눈치를 씁니다.
english.RestartApps=Apps that are already open use the new Nunchi once they are restarted.
korean.OpenSettings=눈치 설정 열기
english.OpenSettings=Open Nunchi's settings

[Tasks]
Name: "defaultime"; Description: "{cm:DefaultIme}"

[Files]
; Setup won't run while a file it installs or deletes ([InstallDelete]) is to be deleted at
; restart. So no uninsrestartdelete, which queues the file's own name: uninstall sets what apps
; still have open aside and queues that (UninstallNeedRestart), and the copies set aside are
; cleaned up in [Code], not [InstallDelete].
Source: "{#Root}\target\release\nunchi_tsf.dll"; DestDir: "{app}"; Flags: ignoreversion regserver 64bit
Source: "{#Root}\target\i686-pc-windows-msvc\release\nunchi_tsf.dll"; DestDir: "{app}\x86"; Flags: ignoreversion regserver 32bit
; The settings window, beside the input method: the taskbar icon's menu opens it from there.
Source: "{#Root}\target\release\nunchi-settings.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "{#Root}\THIRD-PARTY.md"; DestDir: "{app}"; DestName: "THIRD-PARTY.txt"
Source: "{#Root}\data\licenses\*"; DestDir: "{app}\licenses"
; The Rust crates' notices (tools\crate-licenses.ps1, run by build-installer.ps1).
Source: "{#Root}\target\licenses\rust-crates.txt"; DestDir: "{app}\licenses"

[Icons]
Name: "{autoprograms}\{cm:SettingsShortcut}"; Filename: "{app}\nunchi-settings.exe"

[Run]
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\nunchi_tsf.dll"",AddToKeyboardList default"; Flags: runasoriginaluser runhidden waituntilterminated; Tasks: defaultime
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\nunchi_tsf.dll"",AddToKeyboardList"; Flags: runasoriginaluser runhidden waituntilterminated; Tasks: not defaultime
Filename: "{app}\nunchi-settings.exe"; Description: "{cm:OpenSettings}"; Flags: postinstall nowait skipifsilent unchecked runasoriginaluser

[UninstallRun]
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\nunchi_tsf.dll"",RemoveFromKeyboardList"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveFromKeyboardList"

[Code]
var
  DeletesAtRestart, LeftDeleted: Boolean;

function AsideName(Path: String): String;
begin
  Result := Path + '.old-' + GetDateTimeString('yyyymmddhhnnsszzz', #0, #0);
end;

{ A DLL an app has loaded can't be overwritten, but it can be renamed. }
procedure SetAside(Path: String);
begin
  if FileExists(Path) then
    RenameFile(Path, AsideName(Path));
end;

{ Copies set aside earlier, when no app has them open any more. }
procedure DeleteSetAside(Dir: String);
var
  Found: TFindRec;
begin
  if FindFirst(Dir + '\*.old-*', Found) then
  try
    repeat
      DeleteFile(Dir + '\' + Found.Name);
    until not FindNext(Found);
  finally
    FindClose(Found);
  end;
end;

{ What apps still have open in Dir: set aside (so the name is free for installing again) and
  deleted at restart, then Dir itself. }
procedure DeleteAtRestart(Dir: String);
var
  Found: TFindRec;
  Path, Aside: String;
begin
  if not DirExists(Dir) then
    Exit;
  if FindFirst(Dir + '\*', Found) then
  try
    repeat
      if Found.Attributes and FILE_ATTRIBUTE_DIRECTORY = 0 then
      begin
        Path := Dir + '\' + Found.Name;
        if Pos('.old-', Found.Name) = 0 then
        begin
          Aside := AsideName(Path);
          if RenameFile(Path, Aside) then
            Path := Aside;
        end;
        if not DeleteFile(Path) then
        try
          RestartReplace(Path, '');
          DeletesAtRestart := True;
        except
          Log('Could not delete at restart: ' + Path);
        end;
      end;
    until not FindNext(Found);
  finally
    FindClose(Found);
  end;
  if not RemoveDir(Dir) then
  try
    RestartReplace(Dir, '');
  except
    Log('Could not delete at restart: ' + Dir);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    DeleteSetAside(ExpandConstant('{app}'));
    DeleteSetAside(ExpandConstant('{app}\x86'));
    SetAside(ExpandConstant('{app}\nunchi_tsf.dll'));
    SetAside(ExpandConstant('{app}\x86\nunchi_tsf.dll'));
    { The settings window may be open too: a running program can be renamed as well. }
    SetAside(ExpandConstant('{app}\nunchi-settings.exe'));
  end;
end;

procedure DeleteLeftAtRestart();
begin
  if not LeftDeleted then
  begin
    LeftDeleted := True;
    DeleteAtRestart(ExpandConstant('{app}\x86'));
    DeleteAtRestart(ExpandConstant('{app}'));
  end;
end;

{ Asked right after the files are removed, before Uninstall says how it went: with what's left
  deleted at restart, it asks to restart rather than saying some files couldn't be removed. }
function UninstallNeedRestart(): Boolean;
begin
  DeleteLeftAtRestart();
  Result := DeletesAtRestart;
end;

{ Not asked when Uninstall already needs a restart for something else. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    DeleteLeftAtRestart();
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo,
  MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + MemoTasksInfo + NewLine + NewLine + CustomMessage('RestartApps');
end;
