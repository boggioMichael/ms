; MapleSyrup's installer (Inno Setup 6).
;
; Installs for the current user only (no administrator needed), under
; %LOCALAPPDATA%\Programs\MapleSyrup, everything MapleSyrup works with: asks
; for the API keys (OpenAI, ElevenLabs, xAI; each optional, kept in
; %APPDATA%\MapleSyrup where MapleSyrup reads them), installs Claude Code when
; the PC doesn't have it and writes the Claude folder, adds MapleSyrup to the
; Claude desktop app when it is there; one desktop icon and Start menu entry
; start it all (Claude, the voices, the phone over the internet: a Cloudflare
; tunnel, any network); Start menu entries for MapleSyrup without Claude, on
; the same Wi-Fi, for recording and streaming, the sessions folder, read me,
; uninstall; keeps sessions in Documents\MapleSyrup sessions.
; Installing again over it updates it (a running MapleSyrup is closed first).
; Uninstalling leaves the user's settings, key, what it learned and the
; sessions in place.
;
; Built by CI:  ISCC /DAppVersion=0.8.0 /DSourceDir=<the package folder> MapleSyrup.iss

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
; Windows' own language when Setup has it; asks only when it doesn't.
ShowLanguageDialog=auto

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
; The others Inno Setup comes with (Setup picks the one Windows uses).
#if FileExists(AddBackslash(CompilerPath) + "Languages\Hebrew.isl")
Name: "hebrew"; MessagesFile: "compiler:Languages\Hebrew.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Spanish.isl")
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\BrazilianPortuguese.isl")
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\French.isl")
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\German.isl")
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Korean.isl")
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Japanese.isl")
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Russian.isl")
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
#endif

[CustomMessages]
; English, and any language without its own words below.
; The keys page.
KeysTitle=Your API keys
KeysHeading=They make MapleSyrup talk like a person, in the voices you pick on the phone
KeysText=Paste the keys you have. They stay on this PC, in your user folder. Leave a field empty to skip it, or to keep the key this PC has already.%nOpenAI: platform.openai.com/api-keys · ElevenLabs: elevenlabs.io → Developers → API keys · xAI: console.x.ai
KeyOpenAI=OpenAI (sk-...): talking, and seeing your screen
KeyEleven=ElevenLabs (sk_...): the voices on the phone
KeyXai=xAI (xai-...), optional: Grok, quicker answers
KeySaved=[saved already: empty keeps it]
KeyWrong=That doesn't look like an %1 API key (they start with %2). Fix it, or leave the field empty.
; Claude.
ClaudeGroup=Claude:
TaskClaude=Install Claude Code and connect it to MapleSyrup (needs a Claude Pro or Max account)
TaskClaudeApp=Add MapleSyrup to the Claude desktop app, if it's on this PC
SettingUpClaude=Setting up Claude...
InstallingClaude=Installing Claude Code (about a minute)...
; Start menu entries (PhoneOverInternet and WithClaude are 0.9.0's: removed on an update).
WithoutClaude=MapleSyrup without Claude
SameWifi=MapleSyrup (phone on the same Wi-Fi)
PhoneOverInternet=MapleSyrup (phone over the internet)
Recording=MapleSyrup (recording and streaming)
Sessions=MapleSyrup sessions
ReadMe=Read me
WithClaude=MapleSyrup with Claude
AddToClaude=Add MapleSyrup to the Claude app
#if FileExists(AddBackslash(CompilerPath) + "Languages\Hebrew.isl")
hebrew.KeysTitle=מפתחות ה-API שלך
hebrew.KeysHeading=בזכותם MapleSyrup מדבר כמו בן אדם, בקולות שתבחר בטלפון
hebrew.KeysText=הדבק את המפתחות שיש לך. הם נשמרים רק במחשב הזה, בתיקיית המשתמש שלך. שדה ריק מדלגים עליו, או שנשאר המפתח שכבר שמור במחשב.%nOpenAI: platform.openai.com/api-keys · ElevenLabs: elevenlabs.io → Developers → API keys · xAI: console.x.ai
hebrew.KeyOpenAI=OpenAI (sk-...): דיבור וראיית המסך
hebrew.KeyEleven=ElevenLabs (sk_...): הקולות בטלפון
hebrew.KeyXai=xAI (xai-...), לא חובה: Grok, תשובות מהירות יותר
hebrew.KeySaved=[כבר שמור: ריק משאיר אותו]
hebrew.KeyWrong=זה לא נראה כמו מפתח API של %1 (הם מתחילים ב-%2). תקן, או השאר את השדה ריק.
hebrew.ClaudeGroup=Claude:
hebrew.TaskClaude=להתקין את Claude Code ולחבר אותו ל-MapleSyrup (צריך חשבון Claude Pro או Max)
hebrew.TaskClaudeApp=להוסיף את MapleSyrup לאפליקציית Claude במחשב, אם היא מותקנת
hebrew.SettingUpClaude=מכין את Claude...
hebrew.InstallingClaude=מתקין את Claude Code (בערך דקה)...
hebrew.WithoutClaude=MapleSyrup בלי Claude
hebrew.SameWifi=MapleSyrup (טלפון באותו Wi-Fi)
hebrew.PhoneOverInternet=MapleSyrup (טלפון דרך האינטרנט)
hebrew.Recording=MapleSyrup (להקלטה ולסטרים)
hebrew.ReadMe=קרא אותי
hebrew.WithClaude=MapleSyrup עם Claude
hebrew.AddToClaude=הוספת MapleSyrup לאפליקציית Claude
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Spanish.isl")
spanish.PhoneOverInternet=MapleSyrup (teléfono por internet)
spanish.Recording=MapleSyrup (grabar y transmitir)
spanish.ReadMe=Léeme
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\BrazilianPortuguese.isl")
brazilianportuguese.PhoneOverInternet=MapleSyrup (celular pela internet)
brazilianportuguese.Recording=MapleSyrup (gravação e transmissão)
brazilianportuguese.ReadMe=Leia-me
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\French.isl")
french.PhoneOverInternet=MapleSyrup (téléphone par Internet)
french.Recording=MapleSyrup (enregistrement et streaming)
french.ReadMe=Lisez-moi
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\German.isl")
german.PhoneOverInternet=MapleSyrup (Handy über das Internet)
german.Recording=MapleSyrup (Aufnahme und Streaming)
german.ReadMe=Liesmich
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Korean.isl")
korean.PhoneOverInternet=MapleSyrup (인터넷으로 휴대폰 연결)
korean.Recording=MapleSyrup (녹화 및 방송)
korean.ReadMe=읽어 보기
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Japanese.isl")
japanese.PhoneOverInternet=MapleSyrup（インターネット経由でスマホ）
japanese.Recording=MapleSyrup（録画・配信用）
japanese.ReadMe=はじめにお読みください
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Russian.isl")
russian.PhoneOverInternet=MapleSyrup (телефон через интернет)
russian.Recording=MapleSyrup (запись и стриминг)
russian.ReadMe=Инструкция
#endif

[Tasks]
Name: "claude"; Description: "{cm:TaskClaude}"; GroupDescription: "{cm:ClaudeGroup}"
Name: "claudeapp"; Description: "{cm:TaskClaudeApp}"; GroupDescription: "{cm:ClaudeGroup}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\MapleSyrup.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\vision_debug.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\README.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\MapleSyrup with Claude.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\Add MapleSyrup to Claude.cmd"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\maplesyrup.ico"; DestDir: "{app}"; Flags: ignoreversion

[Dirs]
Name: "{userdocs}\MapleSyrup sessions"; Flags: uninsneveruninstall

[Icons]
; MapleSyrup with everything: Claude, the voices, the phone over the internet
; (without Claude when that wasn't chosen).
Name: "{group}\MapleSyrup"; Filename: "{app}\MapleSyrup with Claude.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: claude
Name: "{group}\MapleSyrup"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--tunnel"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: not claude
Name: "{group}\{cm:WithoutClaude}"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--tunnel"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: claude
Name: "{group}\{cm:SameWifi}"; Filename: "{app}\MapleSyrup.exe"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:Recording}"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--overlay-on-stream --record --record-mic"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:AddToClaude}"; Filename: "{app}\Add MapleSyrup to Claude.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
Name: "{group}\{cm:Sessions}"; Filename: "{userdocs}\MapleSyrup sessions"
Name: "{group}\{cm:ReadMe}"; Filename: "{app}\README.txt"
Name: "{group}\{cm:UninstallProgram,MapleSyrup}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\MapleSyrup"; Filename: "{app}\MapleSyrup with Claude.cmd"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: desktopicon and claude
Name: "{userdesktop}\MapleSyrup"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--tunnel"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"; Tasks: desktopicon and not claude

[InstallDelete]
; 0.9.0's Start menu entries the ones above replace.
Type: files; Name: "{group}\{cm:PhoneOverInternet}.lnk"
Type: files; Name: "{group}\{cm:WithClaude}.lnk"

[Run]
; The Claude folder beside MapleSyrup (Start Claude.cmd, and what Claude Code
; reads there); the newline piped in answers its "Press Enter to close".
Filename: "{cmd}"; Parameters: "/c echo.| ""{app}\MapleSyrup.exe"" --setup-claude"; StatusMsg: "{cm:SettingUpClaude}"; Flags: runhidden
; Claude Code, when this PC doesn't have it: Anthropic's own installer, for
; this user, in a window of its own (Start Claude.cmd tries again if it
; didn't go in).
Filename: "{cmd}"; Parameters: "/c curl -fsSL https://claude.ai/install.cmd -o ""{tmp}\claude-install.cmd"" && ""{tmp}\claude-install.cmd"""; StatusMsg: "{cm:InstallingClaude}"; Tasks: claude; Check: ShouldInstallClaudeCode
; The Claude desktop app, when it is on this PC: MapleSyrup among its tools
; (nothing happens when it isn't).
Filename: "{app}\MapleSyrup.exe"; Parameters: "--install-claude"; StatusMsg: "{cm:SettingUpClaude}"; Flags: runhidden; Tasks: claudeapp
; At the end: everything (Claude, the voices, the phone over the internet), or
; MapleSyrup alone.
Filename: "{app}\MapleSyrup with Claude.cmd"; Description: "{cm:LaunchProgram,MapleSyrup}"; Flags: nowait postinstall skipifsilent shellexec; Tasks: claude
Filename: "{app}\MapleSyrup.exe"; Parameters: "--tunnel"; Description: "{cm:LaunchProgram,MapleSyrup}"; Flags: nowait postinstall skipifsilent; Tasks: not claude

[UninstallDelete]
Type: files; Name: "{app}\sessions-folder.txt"
; The Claude folder MapleSyrup writes beside itself (--setup-claude).
Type: filesandordirs; Name: "{app}\Claude"
Type: dirifempty; Name: "{app}"

[Code]
var
  KeyPage: TInputQueryWizardPage;

{ The keys, in the page's order: where MapleSyrup reads each (its settings
  folder), what each starts with, and whose it is. }
function KeyFile(Index: Integer): String;
begin
  case Index of
    0: Result := 'openai-key.txt';
    1: Result := 'elevenlabs-key.txt';
  else
    Result := 'xai-key.txt';
  end;
  Result := ExpandConstant('{userappdata}\MapleSyrup\') + Result;
end;

function KeyPrefix(Index: Integer): String;
begin
  case Index of
    0: Result := 'sk-';
    1: Result := 'sk_';
  else
    Result := 'xai-';
  end;
end;

function KeyService(Index: Integer): String;
begin
  case Index of
    0: Result := 'OpenAI';
    1: Result := 'ElevenLabs';
  else
    Result := 'xAI';
  end;
end;

function KeyLooksRight(Index: Integer; Key: String): Boolean;
begin
  Result := (Pos(KeyPrefix(Index), Key) = 1) and (Length(Key) >= 20) and (Pos(' ', Key) = 0);
end;

procedure InitializeWizard();
var
  I: Integer;
begin
  KeyPage := CreateInputQueryPage(wpSelectTasks,
    CustomMessage('KeysTitle'), CustomMessage('KeysHeading'), CustomMessage('KeysText'));
  KeyPage.Add(CustomMessage('KeyOpenAI'), True);
  KeyPage.Add(CustomMessage('KeyEleven'), True);
  KeyPage.Add(CustomMessage('KeyXai'), True);
  { A key this PC has already is kept when its field is left empty. }
  for I := 0 to 2 do
    if FileExists(KeyFile(I)) then
      KeyPage.PromptLabels[I].Caption := KeyPage.PromptLabels[I].Caption + ' ' + CustomMessage('KeySaved');
end;

{ The keys it needs are saved already (an update): nothing to ask. }
function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = KeyPage.ID) and FileExists(KeyFile(0)) and FileExists(KeyFile(1));
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  I: Integer;
  Key: String;
begin
  Result := True;
  if CurPageID = KeyPage.ID then
    for I := 0 to 2 do
    begin
      Key := Trim(KeyPage.Values[I]);
      if (Key <> '') and (not KeyLooksRight(I, Key)) then
      begin
        MsgBox(FmtMessage(CustomMessage('KeyWrong'), [KeyService(I), KeyPrefix(I)]), mbError, MB_OK);
        Result := False;
        Exit;
      end;
    end;
end;

{ Claude Code on this PC already: where Anthropic's installer puts it, or on
  the PATH (npm, WinGet). }
function ClaudeCodeInstalled(): Boolean;
begin
  Result := FileExists(ExpandConstant('{%USERPROFILE}\.local\bin\claude.exe'))
    or (FileSearch('claude.exe', GetEnv('PATH')) <> '')
    or (FileSearch('claude.cmd', GetEnv('PATH')) <> '');
end;

{ Installed in a window of its own: not in a silent install (CI's test). }
function ShouldInstallClaudeCode(): Boolean;
begin
  Result := (not WizardSilent()) and (not ClaudeCodeInstalled());
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  I: Integer;
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
      for I := 0 to 2 do
      begin
        Key := Trim(KeyPage.Values[I]);
        if Key <> '' then
        begin
          ForceDirectories(ExpandConstant('{userappdata}\MapleSyrup'));
          SaveStringToFile(KeyFile(I), Key, False);
        end;
      end;
  end;
end;
