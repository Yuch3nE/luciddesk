; Build only through tools/package.ps1 -Installer so the EXE and Hook match.
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef ProductName
  #define ProductName "LucidDesk"
#endif
#ifndef AppMutexName
  #define AppMutexName "Local\LucidDesk.DesktopSession"
#endif
#ifndef AppWindowClass
  #define AppWindowClass "windows-window.Window"
#endif
#ifndef AppWindowName
  #define AppWindowName "LucidDesk Tray"
#endif
#ifndef ShutdownTimeout
  #define ShutdownTimeout 20000
#endif
#ifndef ProductId
  #define ProductId "{{A059751B-F1E3-4C4A-AB35-A03FB70C3CF4}"
#endif
#ifndef UserDataFolderName
  #define UserDataFolderName "LucidDesk"
#endif
#ifndef InstallerCompression
  #define InstallerCompression "fast"
#endif

[Setup]
AppId={#ProductId}
AppName={#ProductName}
AppVersion={#AppVersion}
LicenseFile=license.rtf
AppPublisher=Yuchen95
AppPublisherURL=https://github.com/Yuch3nE/luciddesk
AppSupportURL=https://github.com/Yuch3nE/luciddesk/issues
AppUpdatesURL=https://github.com/Yuch3nE/luciddesk/releases
DefaultDirName={autopf}\LucidDesk
DefaultGroupName={#ProductName}
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
UsePreviousPrivileges=yes
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
MinVersion=10.0.17763
UsePreviousAppDir=yes
UsePreviousTasks=yes
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\luciddesk.exe
; Running-app checks are handled in PrepareToInstall and InitializeUninstall.
; AppMutex's startup check would prevent offering automatic close before installation.
SetupMutex=Local\LucidDesk.Setup
; Request normal exit of LucidDesk only, using its tray window in PrepareToInstall.
CloseApplications=no
RestartApplications=no
RestartIfNeededByRun=no
WizardStyle=modern
LanguageDetectionMethod=uilanguage
ShowLanguageDialog=yes
UsePreviousLanguage=yes
Compression=lzma2/{#InstallerCompression}
SolidCompression=yes
SetupIconFile=..\app\assets\luciddesk.ico
OutputDir={#OutputPath}
OutputBaseFilename=LucidDesk-{#AppVersion}-windows-x64-setup
VersionInfoVersion={#AppVersion}
VersionInfoProductName=LucidDesk
VersionInfoProductVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"

Name: "chinesetraditional"; MessagesFile: "compiler:Languages\ChineseTraditional.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Explicit payload: ZIP-only guides and maintenance scripts do not belong in the installer.
Source: "{#SourcePath}\luciddesk.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\luciddesk_explorer.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\luciddesk-cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\plugins\*.dll"; DestDir: "{app}\plugins"; Flags: ignoreversion skipifsourcedoesntexist
Source: "{#SourcePath}\skills\luciddesk-control\SKILL.md"; DestDir: "{app}\skills\luciddesk-control"; Flags: ignoreversion
Source: "{#SourcePath}\skills\luciddesk-control\references\desktop-layout.md"; DestDir: "{app}\skills\luciddesk-control\references"; Flags: ignoreversion
Source: "{#SourcePath}\skills\luciddesk-control\references\folder-search.md"; DestDir: "{app}\skills\luciddesk-control\references"; Flags: ignoreversion
Source: "{#SourcePath}\skills\luciddesk-control\references\installation.md"; DestDir: "{app}\skills\luciddesk-control\references"; Flags: ignoreversion
Source: "{#SourcePath}\skills\luciddesk-control\references\plans-and-recovery.md"; DestDir: "{app}\skills\luciddesk-control\references"; Flags: ignoreversion
Source: "{#SourcePath}\skills\luciddesk-control\references\settings-startup.md"; DestDir: "{app}\skills\luciddesk-control\references"; Flags: ignoreversion
Source: "{#SourcePath}\cli.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\protocol.schema.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourcePath}\build.json"; DestDir: "{app}"; Flags: ignoreversion
Source: "installed"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; Keep this stable across upgrades and in sync with app/src/main.rs.
Name: "{autoprograms}\{#ProductName}"; Filename: "{app}\luciddesk.exe"; WorkingDir: "{app}"; AppUserModelID: "Yuchen95.LucidDesk"
Name: "{autodesktop}\{#ProductName}"; Filename: "{app}\luciddesk.exe"; WorkingDir: "{app}"; AppUserModelID: "Yuchen95.LucidDesk"; Tasks: desktopicon

[Run]
Filename: "{app}\luciddesk.exe"; Description: "{cm:LaunchProgram,LucidDesk}"; Flags: nowait postinstall skipifsilent runasoriginaluser

[CustomMessages]
english.MsiInstalled=The MSI edition is installed. Uninstall it first and keep your settings before switching to EXE.
chinesesimplified.MsiInstalled=已安装 MSI 版。请先卸载并保留配置，再切换到 EXE 版。
english.NewerInstalled=A newer version of LucidDesk is already installed. Installation canceled.
chinesesimplified.NewerInstalled=已安装更新版本的 LucidDesk，安装已取消。
english.PortableDirectory=This folder contains a portable LucidDesk installation. Choose a different folder to preserve portable mode.
chinesesimplified.PortableDirectory=此目录包含便携版 LucidDesk。请选择其他目录，以保留便携模式。
english.AppRunning=LucidDesk is still running. Exit the app before updating.
chinesesimplified.AppRunning=LucidDesk 仍在运行，请退出程序后再更新。
english.AppRunningUninstall=LucidDesk is still running. Exit the app before uninstalling.
chinesesimplified.AppRunningUninstall=LucidDesk 仍在运行，请退出程序后再卸载。
english.CloseApp=LucidDesk is running. Close it automatically and continue installing? Your settings and pane layouts will be preserved.
chinesesimplified.CloseApp=LucidDesk 正在运行。是否自动退出并继续安装？配置和面板布局将保留。
english.CloseAppUninstall=LucidDesk is running. Close it automatically and continue uninstalling?
chinesesimplified.CloseAppUninstall=LucidDesk 正在运行。是否自动退出并继续卸载？
english.CloseTimeout=LucidDesk has not exited. Installation has stopped. Exit the app and try again.
chinesesimplified.CloseTimeout=LucidDesk 尚未退出，安装已停止。请退出程序后重试。
english.CloseTimeoutUninstall=LucidDesk has not exited. Uninstallation has stopped. Exit the app and try again.
chinesesimplified.CloseTimeoutUninstall=LucidDesk 尚未退出，卸载已停止。请退出程序后重试。
english.ComponentBusy=Explorer still has a desktop component loaded. Application files have not been replaced. Exit LucidDesk and wait; if this persists, restart Windows Explorer to release the legacy component, then retry.
chinesesimplified.ComponentBusy=Explorer 中仍有桌面组件，程序文件尚未替换。请退出 LucidDesk 并稍候；若仍未释放，请重启 Windows 资源管理器后重新安装。
english.ComponentInspectionFailed=The installer could not confirm that Explorer released its desktop component. Installation has stopped before replacing application files. Restart Windows Explorer and try again.
chinesesimplified.ComponentInspectionFailed=无法确认 Explorer 桌面组件已释放，程序文件尚未替换。请重启 Windows 资源管理器后重试。
english.ComponentBusyUninstall=Explorer still has a desktop component loaded. Application files have not been deleted. Exit LucidDesk and wait; if this persists, restart Windows Explorer, then retry uninstalling.
chinesesimplified.ComponentBusyUninstall=Explorer 中仍有桌面组件，程序文件尚未删除。请退出 LucidDesk 并稍候；若仍未释放，请重启 Windows 资源管理器后重新卸载。
english.ComponentInspectionFailedUninstall=The uninstaller could not confirm that Explorer released its desktop component. Application files have not been deleted. Restart Windows Explorer and try again.
chinesesimplified.ComponentInspectionFailedUninstall=无法确认 Explorer 桌面组件已释放，程序文件尚未删除。请重启 Windows 资源管理器后重新卸载。
english.KeepUserData=Keep user settings
chinesesimplified.KeepUserData=保留用户配置
english.UserDataDeleteFailed=LucidDesk was uninstalled, but some user data could not be deleted. Check these folders:
chinesesimplified.UserDataDeleteFailed=LucidDesk 已卸载，但部分用户配置未能删除。请检查以下目录：

chinesetraditional.MsiInstalled=已安裝 MSI 版。請先解除安裝並保留設定，再切換至 EXE 版。
chinesetraditional.NewerInstalled=已安裝較新版本的 LucidDesk，安裝已取消。
chinesetraditional.PortableDirectory=此資料夾包含可攜式 LucidDesk。請選擇其他資料夾，以保留可攜模式。
chinesetraditional.AppRunning=LucidDesk 仍在執行，請結束程式後再更新。
chinesetraditional.AppRunningUninstall=LucidDesk 仍在執行，請結束程式後再解除安裝。
chinesetraditional.CloseApp=LucidDesk 正在執行。是否自動結束並繼續安裝？設定與面板配置將會保留。
chinesetraditional.CloseAppUninstall=LucidDesk 正在執行。是否自動結束並繼續解除安裝？
chinesetraditional.CloseTimeout=LucidDesk 尚未結束，安裝已停止。請結束程式後重試。
chinesetraditional.CloseTimeoutUninstall=LucidDesk 尚未結束，解除安裝已停止。請結束程式後重試。
chinesetraditional.ComponentBusy=檔案總管仍載入桌面元件，程式檔案尚未替換。請結束 LucidDesk 並稍候；若仍未釋放，請重新啟動 Windows 檔案總管後重試安裝。
chinesetraditional.ComponentInspectionFailed=無法確認檔案總管已釋放桌面元件，安裝已停止，程式檔案尚未替換。請重新啟動 Windows 檔案總管後重試。
chinesetraditional.ComponentBusyUninstall=檔案總管仍載入桌面元件，程式檔案尚未刪除。請結束 LucidDesk 並稍候；若仍未釋放，請重新啟動 Windows 檔案總管後重試解除安裝。
chinesetraditional.ComponentInspectionFailedUninstall=無法確認檔案總管已釋放桌面元件，程式檔案尚未刪除。請重新啟動 Windows 檔案總管後重試解除安裝。
chinesetraditional.KeepUserData=保留使用者設定
chinesetraditional.UserDataDeleteFailed=LucidDesk 已解除安裝，但部分使用者資料無法刪除。請檢查下列資料夾：

japanese.MsiInstalled=MSI 版がインストールされています。EXE 版に切り替える前に、設定を保持して MSI 版をアンインストールしてください。
japanese.NewerInstalled=新しいバージョンの LucidDesk が既にインストールされています。インストールをキャンセルしました。
japanese.PortableDirectory=このフォルダーにはポータブル版の LucidDesk があります。ポータブルモードを維持するため、別のフォルダーを選択してください。
japanese.AppRunning=LucidDesk はまだ実行中です。アプリを終了してから更新してください。
japanese.AppRunningUninstall=LucidDesk はまだ実行中です。アプリを終了してからアンインストールしてください。
japanese.CloseApp=LucidDesk は実行中です。自動的に終了してインストールを続行しますか？設定とパネルのレイアウトは保持されます。
japanese.CloseAppUninstall=LucidDesk は実行中です。自動的に終了してアンインストールを続行しますか？
japanese.CloseTimeout=LucidDesk が終了していないため、インストールを停止しました。アプリを終了してから再試行してください。
japanese.CloseTimeoutUninstall=LucidDesk が終了していないため、アンインストールを停止しました。アプリを終了してから再試行してください。
japanese.ComponentBusy=エクスプローラーにデスクトップコンポーネントがまだ読み込まれています。プログラムファイルは置き換えていません。LucidDesk を終了してしばらく待ち、解決しない場合は Windows エクスプローラーを再起動してからインストールを再試行してください。
japanese.ComponentInspectionFailed=エクスプローラーがデスクトップコンポーネントを解放したことを確認できません。ファイルの置き換え前にインストールを停止しました。Windows エクスプローラーを再起動してから再試行してください。
japanese.ComponentBusyUninstall=エクスプローラーにデスクトップコンポーネントがまだ読み込まれています。プログラムファイルは削除していません。LucidDesk を終了してしばらく待ち、解決しない場合は Windows エクスプローラーを再起動してからアンインストールを再試行してください。
japanese.ComponentInspectionFailedUninstall=エクスプローラーがデスクトップコンポーネントを解放したことを確認できません。プログラムファイルは削除していません。Windows エクスプローラーを再起動してからアンインストールを再試行してください。
japanese.KeepUserData=ユーザー設定を保持する
japanese.UserDataDeleteFailed=LucidDesk はアンインストールされましたが、一部のユーザーデータを削除できませんでした。次のフォルダーを確認してください：

korean.MsiInstalled=MSI 버전이 설치되어 있습니다. EXE 버전으로 전환하기 전에 설정을 유지한 채 MSI 버전을 제거하세요.
korean.NewerInstalled=더 최신 버전의 LucidDesk가 이미 설치되어 있습니다. 설치가 취소되었습니다.
korean.PortableDirectory=이 폴더에는 LucidDesk 포터블 버전이 있습니다. 포터블 모드를 유지하려면 다른 폴더를 선택하세요.
korean.AppRunning=LucidDesk가 아직 실행 중입니다. 앱을 종료한 후 업데이트하세요.
korean.AppRunningUninstall=LucidDesk가 아직 실행 중입니다. 앱을 종료한 후 제거하세요.
korean.CloseApp=LucidDesk가 실행 중입니다. 자동으로 종료하고 설치를 계속할까요? 설정과 패널 배치는 유지됩니다.
korean.CloseAppUninstall=LucidDesk가 실행 중입니다. 자동으로 종료하고 제거를 계속할까요?
korean.CloseTimeout=LucidDesk가 종료되지 않아 설치가 중단되었습니다. 앱을 종료한 후 다시 시도하세요.
korean.CloseTimeoutUninstall=LucidDesk가 종료되지 않아 제거가 중단되었습니다. 앱을 종료한 후 다시 시도하세요.
korean.ComponentBusy=파일 탐색기에 데스크톱 구성 요소가 아직 로드되어 있습니다. 프로그램 파일은 교체되지 않았습니다. LucidDesk를 종료하고 잠시 기다리세요. 문제가 계속되면 Windows 탐색기를 다시 시작한 후 설치를 다시 시도하세요.
korean.ComponentInspectionFailed=파일 탐색기가 데스크톱 구성 요소를 해제했는지 확인할 수 없습니다. 프로그램 파일을 교체하기 전에 설치가 중단되었습니다. Windows 탐색기를 다시 시작한 후 다시 시도하세요.
korean.ComponentBusyUninstall=파일 탐색기에 데스크톱 구성 요소가 아직 로드되어 있습니다. 프로그램 파일은 삭제되지 않았습니다. LucidDesk를 종료하고 잠시 기다리세요. 문제가 계속되면 Windows 탐색기를 다시 시작한 후 제거를 다시 시도하세요.
korean.ComponentInspectionFailedUninstall=파일 탐색기가 데스크톱 구성 요소를 해제했는지 확인할 수 없습니다. 프로그램 파일은 삭제되지 않았습니다. Windows 탐색기를 다시 시작한 후 제거를 다시 시도하세요.
korean.KeepUserData=사용자 설정 유지
korean.UserDataDeleteFailed=LucidDesk가 제거되었지만 일부 사용자 데이터를 삭제하지 못했습니다. 다음 폴더를 확인하세요:

german.MsiInstalled=Die MSI-Version ist installiert. Deinstallieren Sie diese zuerst unter Beibehaltung Ihrer Einstellungen, bevor Sie zur EXE-Version wechseln.
german.NewerInstalled=Eine neuere Version von LucidDesk ist bereits installiert. Die Installation wurde abgebrochen.
german.PortableDirectory=Dieser Ordner enthält eine portable Version von LucidDesk. Wählen Sie einen anderen Ordner, um den portablen Modus beizubehalten.
german.AppRunning=LucidDesk wird noch ausgeführt. Beenden Sie die Anwendung vor dem Aktualisieren.
german.AppRunningUninstall=LucidDesk wird noch ausgeführt. Beenden Sie die Anwendung vor dem Deinstallieren.
german.CloseApp=LucidDesk wird ausgeführt. Automatisch beenden und die Installation fortsetzen? Ihre Einstellungen und Panel-Anordnungen bleiben erhalten.
german.CloseAppUninstall=LucidDesk wird ausgeführt. Automatisch beenden und die Deinstallation fortsetzen?
german.CloseTimeout=LucidDesk wurde noch nicht beendet. Die Installation wurde gestoppt. Beenden Sie die Anwendung und versuchen Sie es erneut.
german.CloseTimeoutUninstall=LucidDesk wurde noch nicht beendet. Die Deinstallation wurde gestoppt. Beenden Sie die Anwendung und versuchen Sie es erneut.
german.ComponentBusy=Im Explorer ist noch eine Desktop-Komponente geladen. Die Programmdateien wurden nicht ersetzt. Beenden Sie LucidDesk und warten Sie kurz. Starten Sie bei anhaltenden Problemen den Windows-Explorer neu und wiederholen Sie die Installation.
german.ComponentInspectionFailed=Es konnte nicht bestätigt werden, dass der Explorer die Desktop-Komponente freigegeben hat. Die Installation wurde vor dem Ersetzen der Programmdateien gestoppt. Starten Sie den Windows-Explorer neu und versuchen Sie es erneut.
german.ComponentBusyUninstall=Im Explorer ist noch eine Desktop-Komponente geladen. Die Programmdateien wurden nicht gelöscht. Beenden Sie LucidDesk und warten Sie kurz. Starten Sie bei anhaltenden Problemen den Windows-Explorer neu und wiederholen Sie die Deinstallation.
german.ComponentInspectionFailedUninstall=Es konnte nicht bestätigt werden, dass der Explorer die Desktop-Komponente freigegeben hat. Die Programmdateien wurden nicht gelöscht. Starten Sie den Windows-Explorer neu und wiederholen Sie die Deinstallation.
german.KeepUserData=Benutzereinstellungen beibehalten
german.UserDataDeleteFailed=LucidDesk wurde deinstalliert, aber einige Benutzerdaten konnten nicht gelöscht werden. Prüfen Sie diese Ordner:

russian.MsiInstalled=Установлена MSI-версия. Перед переходом на EXE-версию удалите её, сохранив настройки.
russian.NewerInstalled=Уже установлена более новая версия LucidDesk. Установка отменена.
russian.PortableDirectory=В этой папке находится портативная версия LucidDesk. Выберите другую папку, чтобы сохранить портативный режим.
russian.AppRunning=LucidDesk всё ещё работает. Закройте приложение перед обновлением.
russian.AppRunningUninstall=LucidDesk всё ещё работает. Закройте приложение перед удалением.
russian.CloseApp=LucidDesk работает. Закрыть приложение автоматически и продолжить установку? Настройки и расположение панелей будут сохранены.
russian.CloseAppUninstall=LucidDesk работает. Закрыть приложение автоматически и продолжить удаление?
russian.CloseTimeout=LucidDesk ещё не закрыт. Установка остановлена. Закройте приложение и повторите попытку.
russian.CloseTimeoutUninstall=LucidDesk ещё не закрыт. Удаление остановлено. Закройте приложение и повторите попытку.
russian.ComponentBusy=В Проводнике всё ещё загружен компонент рабочего стола. Файлы программы не заменены. Закройте LucidDesk и немного подождите. Если проблема не исчезнет, перезапустите Проводник Windows и повторите установку.
russian.ComponentInspectionFailed=Не удалось подтвердить, что Проводник выгрузил компонент рабочего стола. Установка остановлена до замены файлов программы. Перезапустите Проводник Windows и повторите попытку.
russian.ComponentBusyUninstall=В Проводнике всё ещё загружен компонент рабочего стола. Файлы программы не удалены. Закройте LucidDesk и немного подождите. Если проблема не исчезнет, перезапустите Проводник Windows и повторите удаление.
russian.ComponentInspectionFailedUninstall=Не удалось подтвердить, что Проводник выгрузил компонент рабочего стола. Файлы программы не удалены. Перезапустите Проводник Windows и повторите удаление.
russian.KeepUserData=Сохранить настройки пользователя
russian.UserDataDeleteFailed=LucidDesk удалён, но некоторые пользовательские данные удалить не удалось. Проверьте следующие папки:

[Code]
var
  DeleteUserData: Boolean;

#include "shortcut-cleanup.iss"

function UserDataDirectory(Name: String): String;
var
  Base: String;
begin
  Base := ExpandFileName(ExpandConstant('{localappdata}'));
  Result := ExpandFileName(AddBackslash(Base) + Name);
  // Only this direct child of LocalAppData is eligible for cleanup.
  if (CompareText(ExtractFileDir(Result), Base) <> 0) or
     (Name <> '{#UserDataFolderName}') then
    RaiseException('Invalid LucidDesk user data directory.');
end;

function UserDataDirectoryList: String;
begin
  Result := UserDataDirectory('{#UserDataFolderName}');
end;

function HasArgument(Value: String): Boolean;
var
  Index: Integer;
begin
  Result := False;
  for Index := 1 to ParamCount do
    if CompareText(ParamStr(Index), Value) = 0 then begin
      Result := True;
      Exit;
    end;
end;

type
  TConfirmationRect = record
    Left, Top, Right, Bottom: Integer;
  end;

var
  ConfirmationHook: THandle;
  ConfirmationWindow, KeepDataCheckbox: HWND;
  PreviousConfirmationProc: Longint;

function SetConfirmationHook(Id: Integer; Callback: Longint; Module: THandle; Thread: Cardinal): THandle;
  external 'SetWindowsHookExW@user32.dll stdcall';
function RemoveConfirmationHook(Hook: THandle): Boolean;
  external 'UnhookWindowsHookEx@user32.dll stdcall';
function NextConfirmationHook(Hook: THandle; Code: Integer; WParam: LongWord; LParam: Longint): Longint;
  external 'CallNextHookEx@user32.dll stdcall';
function ConfirmationThread: Cardinal;
  external 'GetCurrentThreadId@kernel32.dll stdcall';
function ConfirmationControl(Wnd: HWND; Id: Integer): HWND;
  external 'GetDlgItem@user32.dll stdcall';
function ConfirmationRect(Wnd: HWND; var Rect: TConfirmationRect): Boolean;
  external 'GetWindowRect@user32.dll stdcall';
function MapConfirmationRect(FromWnd, ToWnd: HWND; var Rect: TConfirmationRect; Count: Cardinal): Integer;
  external 'MapWindowPoints@user32.dll stdcall';
function CreateKeepDataCheckbox(ExStyle: Cardinal; ClassName, Caption: String; Style: Cardinal;
  X, Y, Width, Height: Integer; Parent: HWND; Menu, Instance: THandle; Param: Longint): HWND;
  external 'CreateWindowExW@user32.dll stdcall';
function ConfirmationMessage(Wnd: HWND; Msg: Cardinal; WParam: LongWord; LParam: Longint): Longint;
  external 'SendMessageW@user32.dll stdcall';
function SetConfirmationProc(Wnd: HWND; Index: Integer; Proc: Longint): Longint;
  external 'SetWindowLongW@user32.dll stdcall';
function CallConfirmationProc(Proc: Longint; Wnd: HWND; Msg: Cardinal; WParam: LongWord; LParam: Longint): Longint;
  external 'CallWindowProcW@user32.dll stdcall';
function ConfirmationBackground(Color: Integer): THandle;
  external 'GetSysColorBrush@user32.dll stdcall';
function TransparentConfirmationText(DC: THandle; Mode: Integer): Integer;
  external 'SetBkMode@gdi32.dll stdcall';

function ConfirmationWindowProc(Wnd: HWND; Msg: Cardinal; WParam: LongWord; LParam: Longint): Longint;
var
  Checked: Longint;
begin
  if (Msg = $0111) and ((WParam and $FFFF) = IDYES) and (KeepDataCheckbox <> 0) then begin
    // Read before the native dialog closes; destruction of child controls is too late.
    Checked := ConfirmationMessage(KeepDataCheckbox, $00F0, 0, 0); // BM_GETCHECK
    DeleteUserData := Checked = 0;
    Log(Format('Uninstall confirmation keep-settings state: %d.', [Checked]));
  end;
  if (Msg = $0138) and (LParam = KeepDataCheckbox) and (KeepDataCheckbox <> 0) then begin
    TransparentConfirmationText(WParam, 1); // WM_CTLCOLORSTATIC / TRANSPARENT
    Result := ConfirmationBackground(15); // COLOR_BTNFACE: native confirmation footer
  end else
    Result := CallConfirmationProc(PreviousConfirmationProc, Wnd, Msg, WParam, LParam);
end;

function ConfirmationHookProc(Code: Integer; WParam: LongWord; LParam: Longint): Longint;
var
  YesButton, NoButton: HWND;
  YesRect: TConfirmationRect;
  Margin: Integer;
begin
  // A thread-local CBT hook adds one checkbox to Inno's existing confirmation.
  // It is installed only after shutdown/preflight, never inside Explorer.
  if (Code = 5) and (ConfirmationWindow = 0) then begin // HCBT_ACTIVATE
    YesButton := ConfirmationControl(WParam, IDYES);
    NoButton := ConfirmationControl(WParam, IDNO);
    if (YesButton <> 0) and (NoButton <> 0) and ConfirmationRect(YesButton, YesRect) then begin
      ConfirmationWindow := WParam;
      MapConfirmationRect(0, WParam, YesRect, 2);
      Margin := (YesRect.Bottom - YesRect.Top) div 2;
      KeepDataCheckbox := CreateKeepDataCheckbox(0, 'BUTTON', CustomMessage('KeepUserData'),
        $50010003, Margin, YesRect.Top, YesRect.Left - 2 * Margin,
        YesRect.Bottom - YesRect.Top, WParam, 1001, 0, 0);
      if KeepDataCheckbox <> 0 then begin
        PreviousConfirmationProc := SetConfirmationProc(WParam, -4,
          CreateCallback(@ConfirmationWindowProc)); // GWL_WNDPROC, only this confirmation
        ConfirmationMessage(KeepDataCheckbox, $0030,
          ConfirmationMessage(YesButton, $0031, 0, 0), 1); // WM_SETFONT / WM_GETFONT
        ConfirmationMessage(KeepDataCheckbox, $00F1, 1, 0); // BM_SETCHECK: default keep
      end else
        Log('Could not add the keep-settings checkbox; settings will be retained.');
    end;
  end;
  if (Code = 4) and (WParam = KeepDataCheckbox) and (KeepDataCheckbox <> 0) then begin // HCBT_DESTROYWND
    KeepDataCheckbox := 0;
  end;
  Result := NextConfirmationHook(ConfirmationHook, Code, WParam, LParam);
end;

procedure DeinitializeUninstall;
begin
  if ConfirmationHook <> 0 then begin
    RemoveConfirmationHook(ConfirmationHook);
    ConfirmationHook := 0;
  end;
end;

function OperationMessage(Name: String; Uninstalling: Boolean): String;
begin
  if Uninstalling then Name := Name + 'Uninstall';
  Result := CustomMessage(Name);
end;

function CheckHookReleased(Uninstalling: Boolean): String;
var
  ExistingHook: String;
  ComponentIndex: Integer;
  Stream: TFileStream;
  Attempt: Integer;
  ExitCode: Integer;
  Guard: String;
begin
  Result := '';
  // A writable target file is insufficient: a pinned DLL can survive under its
  // original loader path after it was moved or renamed. Inspect Explorer itself.
  if Uninstalling then
    Guard := ExpandConstant('{app}\luciddesk.exe')
  else begin
    ExtractTemporaryFile('luciddesk.exe');
    Guard := ExpandConstant('{tmp}\luciddesk.exe');
  end;
  ExitCode := 2;
  for Attempt := 1 to 40 do begin
    if not Exec(Guard, '--check-desktop-component', ExpandConstant('{tmp}'),
        SW_HIDE, ewWaitUntilTerminated, ExitCode) then begin
      Result := OperationMessage('ComponentInspectionFailed', Uninstalling);
      Exit;
    end;
    if ExitCode = 0 then Break;
    Sleep(250);
  end;
  if ExitCode <> 0 then begin
    Log(Format('Explorer component preflight failed with exit code %d.', [ExitCode]));
    if ExitCode = 1 then Result := OperationMessage('ComponentBusy', Uninstalling)
    else Result := OperationMessage('ComponentInspectionFailed', Uninstalling);
    Exit;
  end;
  // Check both names when upgrading from the previous desktop component.
  for ComponentIndex := 0 to 1 do begin
    if ComponentIndex = 0 then
      ExistingHook := ExpandConstant('{app}\luciddesk_explorer.dll')
    else
      ExistingHook := ExpandConstant('{app}\luciddesk_desktop.dll');
    if FileExists(ExistingHook) then begin
      for Attempt := 1 to 100 do begin
        try
          Stream := TFileStream.Create(ExistingHook, fmOpenReadWrite or fmShareDenyNone);
          Stream.Free;
          Break;
        except
          if Attempt = 100 then begin
            Result := OperationMessage('ComponentBusy', Uninstalling);
            Exit;
          end;
          Sleep(100);
        end;
      end;
    end;
  end;
end;

function FindAppWindow(ClassName, WindowName: String): HWND;
  external 'FindWindowW@user32.dll stdcall';
function AppWindowProcess(Wnd: HWND; var ProcessId: Cardinal): Cardinal;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function OpenAppProcess(Access: Cardinal; Inherit: Boolean; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function WaitForAppProcess(Process: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseAppProcess(Process: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

function AutoCloseAllowed: Boolean;
var
  Index: Integer;
begin
  Result := True;
  for Index := 1 to ParamCount do
    if CompareText(ParamStr(Index), '/NOCLOSEAPPLICATIONS') = 0 then begin
      Result := False;
      Exit;
    end;
end;

function CloseRunningApp(Uninstalling: Boolean; var Declined: Boolean): String;
var
  Wnd: HWND;
  ProcessId, CurrentProcessId, WaitResult: Cardinal;
  Process: THandle;
begin
  Result := '';
  Declined := False;
  if not CheckForMutexes('{#AppMutexName}') then Exit;
  Result := OperationMessage('AppRunning', Uninstalling);
  if not AutoCloseAllowed then Exit;
  Wnd := FindAppWindow('{#AppWindowClass}', '{#AppWindowName}');
  // Older builds or an app still starting may not expose this window. Ask for manual exit.
  if Wnd = 0 then Exit;
  ProcessId := 0;
  AppWindowProcess(Wnd, ProcessId);
  if ProcessId = 0 then Exit;
  Process := OpenAppProcess($00100000, False, ProcessId); // SYNCHRONIZE
  if Process = 0 then Exit;
  try
    // Suppressed prompts consent to normal close; interactive prompts default to No.
    if SuppressibleMsgBox(OperationMessage('CloseApp', Uninstalling), mbConfirmation,
        MB_YESNO or MB_DEFBUTTON2, IDYES) <> IDYES then begin
      Declined := True;
      Exit;
    end;
    WaitResult := WaitForAppProcess(Process, 0);
    if WaitResult = 258 then begin // WAIT_TIMEOUT: the process is still alive
      // The app could have exited while the confirmation was open. Never message a reused HWND.
      CurrentProcessId := 0;
      AppWindowProcess(Wnd, CurrentProcessId);
      if CurrentProcessId <> ProcessId then Exit;
      Log('Requesting normal LucidDesk exit before changing application files.');
      if not PostMessage(Wnd, $0010, 0, 0) then Exit; // WM_CLOSE, only this tray window
      WaitResult := WaitForAppProcess(Process, {#ShutdownTimeout});
    end;
    if WaitResult <> 0 then begin
      Result := OperationMessage('CloseTimeout', Uninstalling);
      Exit;
    end;
    // Another instance may have started while the old one was closing.
    if not CheckForMutexes('{#AppMutexName}') then begin
      Log('LucidDesk process exited; application files may be changed.');
      Result := '';
    end;
  finally
    CloseAppProcess(Process);
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExistingVersion: String;
  Existing, Incoming: Int64;
  Declined: Boolean;
begin
  Result := '';
  if RegValueExists(HKCU64, 'Software\Yuchen95\{#ProductName}', 'InstallFolder') or
     RegValueExists(HKLM64, 'Software\Yuchen95\{#ProductName}', 'InstallFolder') then begin
    Result := CustomMessage('MsiInstalled');
    Exit;
  end;
  if FileExists(ExpandConstant('{app}\portable')) or FileExists(ExpandConstant('{app}\portable.marker')) then begin
    Result := CustomMessage('PortableDirectory');
    Exit;
  end;
  if RegQueryStringValue(HKA, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\' +
       ExpandConstant('{#ProductId}') + '_is1', 'DisplayVersion', ExistingVersion) and
       StrToVersion(ExistingVersion, Existing) and StrToVersion('{#AppVersion}', Incoming) then
    if ComparePackedVersion(Existing, Incoming) > 0 then begin
      Result := CustomMessage('NewerInstalled');
      Exit;
    end;
  if GetVersionNumbersString(ExpandConstant('{app}\luciddesk.exe'), ExistingVersion) and
     StrToVersion(ExistingVersion, Existing) and StrToVersion('{#AppVersion}', Incoming) then
    if ComparePackedVersion(Existing, Incoming) > 0 then begin
      Result := CustomMessage('NewerInstalled');
      Exit;
    end;
  Result := CloseRunningApp(False, Declined);
  if Result = '' then Result := CheckHookReleased(False);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  AppPath: String;
begin
  if CurStep <> ssPostInstall then Exit;
  AppPath := ExpandConstant('{app}\luciddesk.exe');
  // Invalidate this installation's cached Shell icons after replacing the EXE.
  NotifyShortcutPath($2000, $1005, AppPath, 0); // UPDATEITEM, PATHW | FLUSH
  NotifyShortcutPath($1000, $1005, ExtractFileDir(AppPath), 0); // UPDATEDIR
  NotifyShortcutPath($2000, $1005, ExpandConstant('{autoprograms}\{#ProductName}.lnk'), 0);
  NotifyShortcutPath($2000, $1005, ExpandConstant('{autodesktop}\{#ProductName}.lnk'), 0);
end;

function InitializeUninstall: Boolean;
var
  Error: String;
  Declined: Boolean;
begin
  DeleteUserData := False;
  if UninstallSilent then
    DeleteUserData := HasArgument('/DELETEUSERDATA') and not HasArgument('/KEEPUSERDATA');
  Error := CloseRunningApp(True, Declined);
  if Error = '' then Error := CheckHookReleased(True);
  Result := Error = '';
  if not Result and not Declined then
    SuppressibleMsgBox(Error, mbError, MB_OK, IDOK);
  if Result and not UninstallSilent then begin
    ConfirmationHook := SetConfirmationHook(5, CreateCallback(@ConfirmationHookProc), 0, ConfirmationThread);
    if ConfirmationHook = 0 then Log('Keep-settings checkbox unavailable; settings will be retained.');
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Target, Failed, StartupCommand: String;
begin
  if CurUninstallStep = usAppMutexCheck then DeinitializeUninstall;
  if CurUninstallStep = usUninstall then CollectUninstallShortcuts;
  if CurUninstallStep = usPostUninstall then CleanupUninstallShortcuts;
  // Runtime-created startup registration is independent of keeping user data.
  // Never remove another installation's registration or Windows approval state.
  if CurUninstallStep = usPostUninstall then begin
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
        'LucidDesk', StartupCommand) and
        (CompareText(StartupCommand, '"' + ExpandConstant('{app}\luciddesk.exe') + '" --startup') = 0) then begin
      if not RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'LucidDesk') then
        Log('Unable to remove this installation''s current-user startup registration.');
    end;
  end;
  // Run only after the user confirmed the uninstall and application removal completed.
  if (CurUninstallStep <> usPostUninstall) or not DeleteUserData then Exit;
  Failed := '';
  begin
    Target := UserDataDirectory('{#UserDataFolderName}');
    Log('Removing selected LucidDesk user data directory: ' + Target);
    // DelTree removes junctions themselves without following their targets.
    if DirExists(Target) and not DelTree(Target, True, True, True) then
      Failed := Failed + Target + #13#10;
  end;
  if Failed <> '' then
    SuppressibleMsgBox(CustomMessage('UserDataDeleteFailed') + #13#10 + Failed,
      mbError, MB_OK, IDOK);
end;
// Original desktop files and custom/portable data directories are not uninstall targets.
