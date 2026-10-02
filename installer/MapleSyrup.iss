; MapleSyrup's installer (Inno Setup 6).
;
; Installs for the current user only (no administrator needed), under
; %LOCALAPPDATA%\Programs\MapleSyrup; Start menu entries (MapleSyrup, over the
; internet, for recording and streaming, the sessions folder, read me,
; uninstall) and a desktop icon; asks for an OpenAI API key (optional, kept
; in %APPDATA%\MapleSyrup); keeps sessions in Documents\MapleSyrup sessions.
; Installing again over it updates it (a running MapleSyrup is closed first).
; Uninstalling leaves the user's settings, key, what it learned and the
; sessions in place.
;
; Built by CI:  ISCC /DAppVersion=0.3.0 /DSourceDir=<the package folder> MapleSyrup.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\MapleSyrup"
#endif

[Setup]
AppId={{5E8B7C2A-6F1D-4C3E-9A51-3D2B8E7F0C14}
AppName=MapleSyrup
AppVersion={#AppVersion}
AppVerName=MapleSyrup {#AppVersion}
AppPublisher=MapleSyrup
AppPublisherURL=https://github.com/boggioMichael/ms
AppSupportURL=https://github.com/boggioMichael/ms
VersionInfoVersion={#AppVersion}
VersionInfoDescription=MapleSyrup installer
DefaultDirName={localappdata}\Programs\MapleSyrup
DefaultGroupName=MapleSyrup
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
OutputBaseFilename=MapleSyrup-Setup-{#AppVersion}
SetupIconFile=..\assets\maplesyrup.ico
UninstallDisplayIcon={app}\maplesyrup.ico
UninstallDisplayName=MapleSyrup
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1903 or newer: the window capture MapleSyrup uses needs it.
MinVersion=10.0.18362

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "hebrew"; MessagesFile: "compiler:Languages\Hebrew.isl"

[CustomMessages]
english.KeyTitle=OpenAI API key
english.KeyHeading=Optional: lets MapleSyrup talk like ChatGPT, in a natural voice, and see your screen
english.KeyText=Paste an OpenAI API key (from platform.openai.com/api-keys; the account needs a little credit). It stays on this PC, in your user folder.%n%nLeave it empty to add it later (MapleSyrup asks when it starts) or to use simple answers and the Windows voice.
english.KeyLabel=API key:
english.KeyWrong=That doesn't look like an OpenAI API key (they start with sk-). Leave it empty to add one later.
english.PhoneOverInternet=MapleSyrup (phone over the internet)
english.Recording=MapleSyrup (recording and streaming)
english.Sessions=MapleSyrup sessions
english.ReadMe=Read me
hebrew.KeyTitle=מפתח OpenAI API
hebrew.KeyHeading=לא חובה: כך MapleSyrup ידבר כמו ChatGPT, בקול טבעי, ויראה את המסך שלך
hebrew.KeyText=הדבק מפתח OpenAI API (מ-platform.openai.com/api-keys; צריך קצת קרדיט בחשבון). הוא נשמר רק במחשב הזה, בתיקיית המשתמש שלך.%n%nאפשר להשאיר ריק ולהוסיף אחר כך (MapleSyrup ישאל כשהוא עולה), או להסתפק בתשובות פשוטות ובקול של Windows.
hebrew.KeyLabel=מפתח:
hebrew.KeyWrong=זה לא נראה כמו מפתח OpenAI (הם מתחילים ב-sk-). אפשר להשאיר ריק ולהוסיף אחר כך.
hebrew.PhoneOverInternet=MapleSyrup (טלפון דרך האינטרנט)
hebrew.Recording=MapleSyrup (להקלטה ולסטרים)
hebrew.Sessions=MapleSyrup sessions
hebrew.ReadMe=קרא אותי

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\MapleSyrup.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\vision_debug.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\maplesyrup.ico"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{userdocs}\MapleSyrup sessions"; Flags: uninsneveruninstall

[Icons]
Name: "{group}\MapleSyrup"; Filename: "{app}\MapleSyrup.exe"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:PhoneOverInternet}"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--tunnel"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:Recording}"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--overlay-on-stream --record-mic"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:Sessions}"; Filename: "{userdocs}\MapleSyrup sessions"
Name: "{group}\{cm:ReadMe}"; Filename: "{app}\README.txt"
Name: "{group}\{cm:UninstallProgram,MapleSyrup}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\MapleSyrup"; Filename: "{app}\MapleSyrup.exe"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\MapleSyrup.exe"; Description: "{cm:LaunchProgram,MapleSyrup}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\sessions-folder.txt"
Type: dirifempty; Name: "{app}"

[Code]
var
  KeyPage: TInputQueryWizardPage;

function KeyFile(): String;
begin
  Result := ExpandConstant('{userappdata}\MapleSyrup\openai-key.txt');
end;

procedure InitializeWizard();
begin
  KeyPage := CreateInputQueryPage(wpSelectTasks,
    CustomMessage('KeyTitle'), CustomMessage('KeyHeading'), CustomMessage('KeyText'));
  KeyPage.Add(CustomMessage('KeyLabel'), True);
end;

{ A key is already saved (an update): don't ask again. }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = KeyPage.ID) and FileExists(KeyFile());
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Key: String;
begin
  Result := True;
  if CurPageID = KeyPage.ID then
  begin
    Key := Trim(KeyPage.Values[0]);
    if (Key <> '') and ((Pos('sk-', Key) <> 1) or (Length(Key) < 20) or (Pos(' ', Key) > 0)) then
    begin
      MsgBox(CustomMessage('KeyWrong'), mbError, MB_OK);
      Result := False;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Key: String;
  Lines: TArrayOfString;
begin
  if CurStep = ssPostInstall then
  begin
    { Where this copy keeps its sessions (UTF-8: user folders can have any name). }
    SetArrayLength(Lines, 1);
    Lines[0] := ExpandConstant('{userdocs}\MapleSyrup sessions');
    SaveStringsToUTF8File(ExpandConstant('{app}\sessions-folder.txt'), Lines, False);
    if not WizardSilent() then
    begin
      Key := Trim(KeyPage.Values[0]);
      if Key <> '' then
      begin
        ForceDirectories(ExpandConstant('{userappdata}\MapleSyrup'));
        SaveStringToFile(KeyFile(), Key, False);
      end;
    end;
  end;
end;
