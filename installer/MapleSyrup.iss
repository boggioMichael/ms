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
; Built by CI:  ISCC /DAppVersion=0.4.0 /DSourceDir=<the package folder> MapleSyrup.iss

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
KeyTitle=OpenAI API key
KeyHeading=Optional: lets MapleSyrup talk like ChatGPT, in a natural voice, and see your screen
KeyText=Paste an OpenAI API key (from platform.openai.com/api-keys; the account needs a little credit). It stays on this PC, in your user folder.%n%nLeave it empty to add it later (MapleSyrup asks when it starts) or to use simple answers and the Windows voice.
KeyLabel=API key:
KeyWrong=That doesn't look like an OpenAI API key (they start with sk-). Leave it empty to add one later.
PhoneOverInternet=MapleSyrup (phone over the internet)
Recording=MapleSyrup (recording and streaming)
Sessions=MapleSyrup sessions
ReadMe=Read me
#if FileExists(AddBackslash(CompilerPath) + "Languages\Hebrew.isl")
hebrew.KeyTitle=מפתח OpenAI API
hebrew.KeyHeading=לא חובה: כך MapleSyrup ידבר כמו ChatGPT, בקול טבעי, ויראה את המסך שלך
hebrew.KeyText=הדבק מפתח OpenAI API (מ-platform.openai.com/api-keys; צריך קצת קרדיט בחשבון). הוא נשמר רק במחשב הזה, בתיקיית המשתמש שלך.%n%nאפשר להשאיר ריק ולהוסיף אחר כך (MapleSyrup ישאל כשהוא עולה), או להסתפק בתשובות פשוטות ובקול של Windows.
hebrew.KeyLabel=מפתח:
hebrew.KeyWrong=זה לא נראה כמו מפתח OpenAI (הם מתחילים ב-sk-). אפשר להשאיר ריק ולהוסיף אחר כך.
hebrew.PhoneOverInternet=MapleSyrup (טלפון דרך האינטרנט)
hebrew.Recording=MapleSyrup (להקלטה ולסטרים)
hebrew.ReadMe=קרא אותי
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Spanish.isl")
spanish.KeyTitle=Clave de API de OpenAI
spanish.KeyHeading=Opcional: MapleSyrup hablará como ChatGPT, con una voz natural, y verá tu pantalla
spanish.KeyText=Pega una clave de API de OpenAI (de platform.openai.com/api-keys; la cuenta necesita un poco de saldo). Se queda en este PC, en tu carpeta de usuario.%n%nDéjala vacía para añadirla más tarde (MapleSyrup la pide al iniciar) o para usar respuestas sencillas y la voz de Windows.
spanish.KeyLabel=Clave de API:
spanish.KeyWrong=Eso no parece una clave de API de OpenAI (empiezan por sk-). Déjala vacía para añadirla más tarde.
spanish.PhoneOverInternet=MapleSyrup (teléfono por internet)
spanish.Recording=MapleSyrup (grabar y transmitir)
spanish.ReadMe=Léeme
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\BrazilianPortuguese.isl")
brazilianportuguese.KeyTitle=Chave de API da OpenAI
brazilianportuguese.KeyHeading=Opcional: o MapleSyrup fala como o ChatGPT, com voz natural, e vê a sua tela
brazilianportuguese.KeyText=Cole uma chave de API da OpenAI (em platform.openai.com/api-keys; a conta precisa de um pouco de crédito). Ela fica neste PC, na sua pasta de usuário.%n%nDeixe em branco para adicionar depois (o MapleSyrup pede ao iniciar) ou para usar respostas simples e a voz do Windows.
brazilianportuguese.KeyLabel=Chave de API:
brazilianportuguese.KeyWrong=Isso não parece uma chave de API da OpenAI (elas começam com sk-). Deixe em branco para adicionar depois.
brazilianportuguese.PhoneOverInternet=MapleSyrup (celular pela internet)
brazilianportuguese.Recording=MapleSyrup (gravação e transmissão)
brazilianportuguese.ReadMe=Leia-me
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\French.isl")
french.KeyTitle=Clé d'API OpenAI
french.KeyHeading=Facultatif : MapleSyrup parle comme ChatGPT, avec une voix naturelle, et voit votre écran
french.KeyText=Collez une clé d'API OpenAI (sur platform.openai.com/api-keys ; le compte a besoin d'un peu de crédit). Elle reste sur ce PC, dans votre dossier utilisateur.%n%nLaissez vide pour l'ajouter plus tard (MapleSyrup la demande au démarrage) ou pour utiliser des réponses simples et la voix de Windows.
french.KeyLabel=Clé d'API :
french.KeyWrong=Cela ne ressemble pas à une clé d'API OpenAI (elles commencent par sk-). Laissez vide pour l'ajouter plus tard.
french.PhoneOverInternet=MapleSyrup (téléphone par Internet)
french.Recording=MapleSyrup (enregistrement et streaming)
french.ReadMe=Lisez-moi
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\German.isl")
german.KeyTitle=OpenAI-API-Schlüssel
german.KeyHeading=Optional: Damit spricht MapleSyrup wie ChatGPT, mit natürlicher Stimme, und sieht Ihren Bildschirm
german.KeyText=Fügen Sie einen OpenAI-API-Schlüssel ein (von platform.openai.com/api-keys; das Konto braucht etwas Guthaben). Er bleibt auf diesem PC, in Ihrem Benutzerordner.%n%nLassen Sie das Feld leer, um ihn später hinzuzufügen (MapleSyrup fragt beim Start danach) oder um einfache Antworten und die Windows-Stimme zu nutzen.
german.KeyLabel=API-Schlüssel:
german.KeyWrong=Das sieht nicht wie ein OpenAI-API-Schlüssel aus (diese beginnen mit sk-). Lassen Sie das Feld leer, um später einen hinzuzufügen.
german.PhoneOverInternet=MapleSyrup (Handy über das Internet)
german.Recording=MapleSyrup (Aufnahme und Streaming)
german.ReadMe=Liesmich
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Korean.isl")
korean.KeyTitle=OpenAI API 키
korean.KeyHeading=선택 사항: MapleSyrup이 ChatGPT처럼 자연스러운 목소리로 말하고 화면을 볼 수 있게 됩니다
korean.KeyText=OpenAI API 키를 붙여 넣으세요 (platform.openai.com/api-keys에서 만들 수 있으며, 계정에 약간의 크레딧이 필요합니다). 키는 이 PC의 사용자 폴더에만 저장됩니다.%n%n나중에 추가하려면 비워 두세요 (MapleSyrup이 시작할 때 물어봅니다). 비워 두면 간단한 대답과 Windows 음성을 사용합니다.
korean.KeyLabel=API 키:
korean.KeyWrong=OpenAI API 키가 아닌 것 같습니다 (키는 sk-로 시작합니다). 나중에 추가하려면 비워 두세요.
korean.PhoneOverInternet=MapleSyrup (인터넷으로 휴대폰 연결)
korean.Recording=MapleSyrup (녹화 및 방송)
korean.ReadMe=읽어 보기
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Japanese.isl")
japanese.KeyTitle=OpenAI API キー
japanese.KeyHeading=任意：MapleSyrup が ChatGPT のように自然な声で話し、画面を見られるようになります
japanese.KeyText=OpenAI API キーを貼り付けてください（platform.openai.com/api-keys で作成できます。アカウントに少しクレジットが必要です）。キーはこの PC のユーザー フォルダーにだけ保存されます。%n%n後で追加する場合は空欄のままにしてください（MapleSyrup の起動時に聞かれます）。空欄のままだと、簡単な返事と Windows の音声を使います。
japanese.KeyLabel=API キー:
japanese.KeyWrong=OpenAI API キーではないようです（キーは sk- で始まります）。後で追加する場合は空欄のままにしてください。
japanese.PhoneOverInternet=MapleSyrup（インターネット経由でスマホ）
japanese.Recording=MapleSyrup（録画・配信用）
japanese.ReadMe=はじめにお読みください
#endif
#if FileExists(AddBackslash(CompilerPath) + "Languages\Russian.isl")
russian.KeyTitle=Ключ API OpenAI
russian.KeyHeading=Необязательно: с ним MapleSyrup говорит как ChatGPT, естественным голосом, и видит ваш экран
russian.KeyText=Вставьте ключ API OpenAI (с platform.openai.com/api-keys; на счёте нужно немного средств). Он хранится только на этом ПК, в вашей папке пользователя.%n%nОставьте поле пустым, чтобы добавить ключ позже (MapleSyrup спросит при запуске) или пользоваться простыми ответами и голосом Windows.
russian.KeyLabel=Ключ API:
russian.KeyWrong=Это не похоже на ключ API OpenAI (они начинаются с sk-). Оставьте поле пустым, чтобы добавить ключ позже.
russian.PhoneOverInternet=MapleSyrup (телефон через интернет)
russian.Recording=MapleSyrup (запись и стриминг)
russian.ReadMe=Инструкция
#endif

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
Name: "{group}\{cm:Recording}"; Filename: "{app}\MapleSyrup.exe"; Parameters: "--overlay-on-stream --record --record-mic"; WorkingDir: "{app}"; IconFilename: "{app}\maplesyrup.ico"
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
