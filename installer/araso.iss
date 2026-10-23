; Araso's installer (Inno Setup 6): the 64-bit and 32-bit input method in Program Files,
; registered, and put in the user's keyboard list. Built by scripts\build-installer.ps1, which
; passes the version (/DAppVersion=…).
;
; Program Files is where sandboxed apps (Store apps) can read an input method from. The DLLs an
; app has loaded can't be overwritten, so an update renames them aside first: apps already open
; keep the old version until they are restarted, apps opened after get the new one.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#define Root ".."

[Setup]
AppId={{0B8DC63D-8A1B-466F-8A28-4AADAB648E0D}
AppName=Araso
AppVersion={#AppVersion}
AppVerName=Araso {#AppVersion}
AppPublisher=mununki
AppPublisherURL=https://github.com/mununki/araso-windows
AppSupportURL=https://github.com/mununki/araso-windows/issues
DefaultDirName={autopf}\Araso
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#Root}\target\installer
OutputBaseFilename=Araso-Setup-{#AppVersion}
SetupIconFile={#Root}\crates\araso-tsf\res\araso.ico
UninstallDisplayIcon={app}\araso_tsf.dll
UninstallDisplayName=Araso
LicenseFile={#Root}\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Every app with a text field may have the DLL loaded: closing them all isn't Setup's business.
CloseApplications=no
RestartApplications=no

[Languages]
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
korean.DefaultIme=Araso를 기본 입력기로 지정
english.DefaultIme=Make Araso the default input method
korean.RestartApps=이미 열려 있는 앱은 다시 시작해야 새로 설치한 Araso를 씁니다.
english.RestartApps=Apps that are already open use the new Araso once they are restarted.

[Tasks]
Name: "defaultime"; Description: "{cm:DefaultIme}"

[InstallDelete]
; Copies an earlier update set aside, when no app has them open any more.
Type: files; Name: "{app}\*.old-*"
Type: files; Name: "{app}\x86\*.old-*"

[Files]
Source: "{#Root}\target\release\araso_tsf.dll"; DestDir: "{app}"; Flags: ignoreversion regserver 64bit uninsrestartdelete
Source: "{#Root}\target\i686-pc-windows-msvc\release\araso_tsf.dll"; DestDir: "{app}\x86"; Flags: ignoreversion regserver 32bit uninsrestartdelete
Source: "{#Root}\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "{#Root}\THIRD-PARTY.md"; DestDir: "{app}"; DestName: "THIRD-PARTY.txt"
Source: "{#Root}\data\licenses\*"; DestDir: "{app}\licenses"

[Run]
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\araso_tsf.dll"",AddToKeyboardList default"; Flags: runasoriginaluser runhidden waituntilterminated; Tasks: defaultime
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\araso_tsf.dll"",AddToKeyboardList"; Flags: runasoriginaluser runhidden waituntilterminated; Tasks: not defaultime

[UninstallRun]
Filename: "{sys}\rundll32.exe"; Parameters: """{app}\araso_tsf.dll"",RemoveFromKeyboardList"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveFromKeyboardList"

[UninstallDelete]
Type: files; Name: "{app}\*.old-*"
Type: files; Name: "{app}\x86\*.old-*"

[Code]
{ A DLL an app has loaded can't be overwritten, but it can be renamed. }
procedure SetAside(Path: String);
begin
  if FileExists(Path) then
    RenameFile(Path, Path + '.old-' + GetDateTimeString('yyyymmddhhnnsszzz', #0, #0));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    SetAside(ExpandConstant('{app}\araso_tsf.dll'));
    SetAside(ExpandConstant('{app}\x86\araso_tsf.dll'));
  end;
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo,
  MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + MemoTasksInfo + NewLine + NewLine + CustomMessage('RestartApps');
end;
