; AuthTile 安装包（Inno Setup 6）。用 scripts\installer.ps1 构建：它先编译，再传入版本号。
; 按当前用户安装到 %LOCALAPPDATA%\Programs\AuthTile，不要管理员权限。

#ifndef Version
  #define Version "0.0.0"
#endif

[Setup]
AppId=AuthTile
AppName=AuthTile
AppVersion={#Version}
AppVerName=AuthTile {#Version}
AppPublisher=ArthurLau
AppPublisherURL=https://github.com/ArthurLauCS/AuthenticatorInDiscord
DefaultDirName={autopf}\AuthTile
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
OutputDir=..\target\installer
OutputBaseFilename=AuthTile-Setup-{#Version}
SetupIconFile=..\tile\assets\app.ico
UninstallDisplayIcon={app}\authtile.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; 每次都让用户选语言；第一次安装时选中的语言也是程序的界面语言
UsePreviousLanguage=no
; 升级时关掉正在运行的实例（验证器和设置一改就已经存盘）
CloseApplications=force
RestartApplications=no

[Languages]
Name: "zh"; MessagesFile: "ChineseSimplified.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
zh.AutoStart=开机自动启动
en.AutoStart=Start with Windows
zh.DeleteData=同时删除设置和导入的验证器吗？%n%n删掉后只能重新导入 Google 验证器的二维码。重装时保留它们可以接着用。
en.DeleteData=Also delete your settings and imported authenticators?%n%nOnce deleted, you would have to import the Google Authenticator QR codes again. Keep them if you plan to reinstall.

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "..\target\release\authtile.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\AuthTile"; Filename: "{app}\authtile.exe"
Name: "{autodesktop}\AuthTile"; Filename: "{app}\authtile.exe"; Tasks: desktopicon

[Registry]
; 和菜单里“开机自启”写的是同一个值，两边开关互认
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "AuthTile"; ValueData: """{app}\authtile.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[Run]
Filename: "{app}\authtile.exe"; Description: "{cm:LaunchProgram,AuthTile}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM authtile.exe"; Flags: runhidden; RunOnceId: "Kill"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

function ConfigFile: String;
begin
  Result := ExpandConstant('{userappdata}\AuthTile\config.txt');
end;

// 第一次安装：把安装时选的语言写进设置；已经有设置（升级、重装）就不动
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and not FileExists(ConfigFile) then
  begin
    ForceDirectories(ExtractFileDir(ConfigFile));
    SaveStringToFile(ConfigFile, 'lang|' + ActiveLanguage + #13#10, False);
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    // 菜单里打开的开机自启（安装时没勾也可能有）一并删掉
    RegDeleteValue(HKCU, RunKey, 'AuthTile');
    // 静默卸载时按默认“否”，保留数据
    if SuppressibleMsgBox(CustomMessage('DeleteData'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2, IDNO) = IDYES then
      DelTree(ExpandConstant('{userappdata}\AuthTile'), True, True, True);
  end;
end;
