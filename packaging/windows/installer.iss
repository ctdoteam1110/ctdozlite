; QTzalo — Windows installer (Inno Setup 6).
;
; Build (from the repository root, after `cargo build --release --bin zalo-rust`):
;   iscc /DMyAppVersion=0.1.0 packaging\windows\installer.iss
; CI passes the version read from Cargo.toml. Without /DMyAppVersion the installer is
; labelled 0.0.0 so a local build can never be mistaken for a release.
;
; User data (session, history, caches) lives in %APPDATA%\QTzalo and is kept on uninstall
; unless the user explicitly agrees to remove it.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#define MyAppName "QTzalo"
#define MyAppPublisher "quangthoai1985"
#define MyAppURL "https://github.com/quangthoai1985/QTzalo"
#define MyAppSourceExe "zalo-rust.exe"
#define MyAppExeName "QTzalo.exe"

[Setup]
; NOTE: AppId identifies this product for upgrades/uninstall. Never reuse it for another app,
; and do not change it between QTzalo versions. (Distinct from upstream ctdozlite's AppId.)
AppId={{D142F24F-C038-41CC-AFB5-8116B000D0CE}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppVerName={#MyAppName} {#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
AppCopyright=Copyright (C) 2026 {#MyAppPublisher} - based on ctdozlite by ctdoteam1110 (MIT OR Apache-2.0)
VersionInfoVersion={#MyAppVersion}
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#MyAppVersion}
VersionInfoDescription={#MyAppName} Setup
VersionInfoCompany={#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=..\..\LICENSE-MIT
OutputDir=..\..\dist
OutputBaseFilename={#MyAppName}-{#MyAppVersion}-setup-windows-x86_64
SetupIconFile=..\..\src\assets\app_icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName} {#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
MinVersion=10.0
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no

[Languages]
; Inno Setup 6 does not ship an official Vietnamese translation, so the wizard is English.
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#MyAppSourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--gui"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Parameters: "--gui"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Parameters: "--gui"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Ask (default: No) whether to also delete the user's data folder on uninstall.
// Silent uninstalls never delete user data.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    DataDir := ExpandConstant('{userappdata}\{#MyAppName}');
    if DirExists(DataDir) and (not UninstallSilent()) then
    begin
      if MsgBox('Xoa ca du lieu nguoi dung cua QTzalo (phien dang nhap, lich su tin nhan, bo nho dem)?' + #13#10 +
                'Thu muc: ' + DataDir + #13#10#13#10 +
                'Also delete QTzalo user data (login session, message history, caches)?',
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      begin
        DelTree(DataDir, True, True, True);
      end;
    end;
  end;
end;
